use crate::ports::CANDIDATE_PORTS;
use anyhow::{anyhow, Result};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use chrono::Local;
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{timeout, Duration, Instant};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

/// Internal item dispatched to a client's writer task.
enum OutFrame {
    Msg(WsMessage),
    Close { code: u16, reason: String },
}

type ClientId = u64;
type ClientSender = mpsc::UnboundedSender<OutFrame>;

struct AppState {
    version: String,
    commit: String,
    next_id: AtomicU64,
    /// Authenticated clients only.
    clients: Mutex<HashMap<ClientId, ClientSender>>,
}

type BroadcastFn = Arc<dyn Fn(WsMessage) + Send + Sync>;

/// Cheap clone-able handle for broadcasting outside the HTTP layer.
#[derive(Clone)]
pub struct Broadcaster {
    inner: BroadcastFn,
}

impl Broadcaster {
    pub fn broadcast(&self, msg: WsMessage) {
        (self.inner)(msg);
    }

    /// Construct a Broadcaster backed by a closure — used by tests.
    pub fn test_sink<F>(f: F) -> Self
    where
        F: Fn(WsMessage) + Send + Sync + 'static,
    {
        Self { inner: Arc::new(f) }
    }

    fn from_state(state: Arc<AppState>) -> Self {
        let inner: BroadcastFn = Arc::new(move |msg: WsMessage| {
            let state = state.clone();
            tokio::spawn(async move {
                let map = state.clients.lock().await;
                tracing::info!(
                    "Broadcasting {:?} to {} client(s)",
                    msg.msg_type,
                    map.len()
                );
                for (_id, tx) in map.iter() {
                    let _ = tx.send(OutFrame::Msg(msg.clone()));
                }
            });
        });
        Self { inner }
    }
}

pub struct Server {
    pub port: u16,
    pub broadcaster: Broadcaster,
    handle: tokio::task::JoinHandle<()>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Server {
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }
}

pub async fn start(version: String, commit: String, debug_mode: bool) -> Result<Server> {
    let state = Arc::new(AppState {
        version,
        commit,
        next_id: AtomicU64::new(1),
        clients: Mutex::new(HashMap::new()),
    });

    let mut app = Router::new()
        .route("/ws", any(ws_handler))
        .route("/health", get(health_handler));
    if debug_mode || std::env::var("GO_TEST").as_deref() == Ok("1") {
        tracing::info!("Debug mode: registering /test/broadcast handler");
        app = app.route("/test/broadcast", post(test_broadcast_handler));
    }
    let app = app.with_state(state.clone());

    let mut last_err: Option<std::io::Error> = None;
    let mut listener_and_port: Option<(TcpListener, u16)> = None;
    for &port in CANDIDATE_PORTS {
        tracing::info!("Trying port {} for WebSocket server", port);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        match TcpListener::bind(addr).await {
            Ok(l) => {
                listener_and_port = Some((l, port));
                break;
            }
            Err(e) => {
                tracing::warn!("Failed to bind port {}: {}", port, e);
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    last_err = Some(e);
                    continue;
                }
                return Err(anyhow!(e));
            }
        }
    }
    let (listener, port) = listener_and_port
        .ok_or_else(|| anyhow!("no free candidate port: {:?}", last_err))?;
    tracing::info!("Using port {} for WebSocket server", port);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        let make = app.into_make_service_with_connect_info::<SocketAddr>();
        let serve = axum::serve(listener, make).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
            tracing::info!("Shutting down WebSocket server");
        });
        if let Err(e) = serve.await {
            tracing::warn!("WebSocket server error: {}", e);
        }
    });

    Ok(Server {
        port,
        broadcaster: Broadcaster::from_state(state),
        handle,
        shutdown: Some(shutdown_tx),
    })
}

async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Number of authenticated WebSocket clients (i.e. connected Chrome extensions).
    let clients = state.clients.lock().await.len();
    let body = json!({
        "status": "ok",
        "version": state.version,
        "commit": state.commit,
        "clients": clients,
        "timestamp": Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    });
    (StatusCode::OK, Json(body))
}

async fn test_broadcast_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> StatusCode {
    let msg_type = body
        .get("type")
        .or_else(|| body.get("Type"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let payload = body
        .get("payload")
        .or_else(|| body.get("Payload"))
        .cloned();
    let Some(msg_type) = msg_type else {
        return StatusCode::BAD_REQUEST;
    };
    let msg = WsMessage { msg_type, payload };
    let bc = Broadcaster::from_state(state);
    bc.broadcast(msg);
    StatusCode::OK
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    tracing::info!("New WebSocket connection attempt from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, state, addr))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, addr: SocketAddr) {
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();

    // Random 16-byte hex token.
    let mut token_bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut token_bytes);
    let token = hex::encode(token_bytes);
    tracing::info!("Generated authentication token for client {}", addr);

    let (tx, mut rx) = mpsc::unbounded_channel::<OutFrame>();

    // Send hello immediately.
    let _ = tx.send(OutFrame::Msg(WsMessage {
        msg_type: "hello".into(),
        payload: Some(json!({
            "token": token,
            "version": state.version,
        })),
    }));
    tracing::info!("Sent hello challenge to client {}", addr);

    // Writer task: drains rx → sink.
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            match frame {
                OutFrame::Msg(msg) => {
                    let body = match serde_json::to_string(&msg) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("serialize ws message: {}", e);
                            continue;
                        }
                    };
                    if let Err(e) = sink.send(Message::Text(body)).await {
                        tracing::warn!("write failed: {}", e);
                        break;
                    }
                }
                OutFrame::Close { code, reason } => {
                    let _ = sink
                        .send(Message::Close(Some(CloseFrame {
                            code,
                            reason: reason.into(),
                        })))
                        .await;
                    break;
                }
            }
        }
        let _ = sink.close().await;
    });

    let mut authed = false;
    let handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;

    loop {
        let deadline = if authed {
            Instant::now() + READ_TIMEOUT
        } else {
            handshake_deadline
        };
        let dur = deadline.saturating_duration_since(Instant::now());

        let next = timeout(dur, stream.next()).await;
        let msg = match next {
            Err(_) => {
                if authed {
                    tracing::warn!("ws read timeout on {}", addr);
                } else {
                    tracing::warn!("handshake timeout {}", addr);
                    let _ = tx.send(OutFrame::Close {
                        code: 1008,
                        reason: "handshake timeout".into(),
                    });
                }
                break;
            }
            Ok(None) => break,
            Ok(Some(Err(e))) => {
                tracing::warn!("ws read: {}", e);
                break;
            }
            Ok(Some(Ok(m))) => m,
        };

        match msg {
            Message::Text(txt) => {
                let parsed: serde_json::Result<WsMessage> = serde_json::from_str(&txt);
                let parsed = match parsed {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("malformed ws message from {}: {}", addr, e);
                        continue;
                    }
                };

                if !authed {
                    if parsed.msg_type != "hello_ack" {
                        tracing::warn!(
                            "Expected hello_ack but got {} from {}",
                            parsed.msg_type,
                            addr
                        );
                        break;
                    }
                    let got_token = parsed
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("token"))
                        .and_then(|v| v.as_str());
                    if got_token != Some(token.as_str()) {
                        tracing::warn!("Invalid token from client {}", addr);
                        break;
                    }
                    authed = true;

                    {
                        let mut map = state.clients.lock().await;
                        map.insert(id, tx.clone());
                        tracing::info!(
                            "Authentication successful for client {} (authenticated clients: {})",
                            addr,
                            map.len()
                        );
                    }

                    let _ = tx.send(OutFrame::Msg(WsMessage {
                        msg_type: "connected".into(),
                        payload: Some(json!({
                            "timestamp": Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        })),
                    }));
                    continue;
                }

                match parsed.msg_type.as_str() {
                    "ping" => {
                        let _ = tx.send(OutFrame::Msg(WsMessage {
                            msg_type: "pong".into(),
                            payload: Some(json!({
                                "timestamp": Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            })),
                        }));
                    }
                    other => {
                        tracing::info!("unknown msg {:?} from {}", other, addr);
                    }
                }
            }
            Message::Binary(_) => {}
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => break,
        }
    }

    {
        let mut map = state.clients.lock().await;
        if map.remove(&id).is_some() {
            tracing::info!(
                "WS client disconnected {} (remaining clients: {})",
                addr,
                map.len()
            );
        }
    }

    drop(tx);
    let _ = writer_task.await;
}
