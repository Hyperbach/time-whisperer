{
  description = "TimeWhisperer — Rust development shell";

  # Release binaries are produced by GitHub Actions with `cargo build --release`
  # (see .github/workflows/release.yml). This flake only provides a dev shell;
  # there is no Nix package build. Build locally with plain `cargo build`.

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-darwin" "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in {
      devShells = forAllSystems (system:
        let pkgs = pkgsFor system;
        in {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              pkg-config
            ];
          };
        }
      );
    };
}
