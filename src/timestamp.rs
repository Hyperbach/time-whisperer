use chrono::{DateTime, Local, NaiveDateTime, TimeZone};

/// Parses the first `[...]` timestamp in a log line.
///
/// Accepts:
///   * Upwork's `2025-05-12T11:26:23.318` (no zone, treated as local time)
///   * Full RFC3339 with or without nanoseconds
pub fn parse_ts(line: &str) -> Option<DateTime<Local>> {
    let start = line.find('[')?;
    let rest = &line[start + 1..];
    let end = rest.find(']')?;
    let s = &rest[..end];

    // Fast path: "YYYY-MM-DDThh:mm:ss.sss"
    if s.len() == "2006-01-02T15:04:05.000".len() {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.3f") {
            if let Some(dt) = Local.from_local_datetime(&naive).single() {
                return Some(dt);
            }
        }
    }

    // RFC3339 (with or without fractional seconds, with timezone)
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Local));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upwork_no_zone_format() {
        let line = "[2025-05-12T11:26:23.318] [INFO] foo";
        let ts = parse_ts(line).expect("should parse");
        assert_eq!(ts.format("%Y-%m-%dT%H:%M:%S%.3f").to_string(), "2025-05-12T11:26:23.318");
    }

    #[test]
    fn parses_rfc3339() {
        let line = "[2025-05-12T11:26:23.318Z] something";
        let ts = parse_ts(line).expect("should parse");
        // Verify it picked something sensible — exact local representation depends on TZ.
        assert!(ts.timestamp() > 0);
    }

    #[test]
    fn returns_none_for_no_brackets() {
        assert!(parse_ts("no brackets here").is_none());
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(parse_ts("[not-a-timestamp]").is_none());
    }
}
