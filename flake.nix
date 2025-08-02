{
  description = "TimeWhisperer reproducible build";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  
  outputs = { self, nixpkgs }:
    let 
      # Define systems we want to support
      supportedSystems = [ "x86_64-darwin" "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      
      # Helper function to generate outputs for each system
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      
      # Get pkgs for each system
      pkgsFor = system: import nixpkgs { inherit system; };
    in {
      packages = forAllSystems (system: 
        let 
          pkgs = pkgsFor system;
          version = "1.0.0"; # This should be automated or passed in
        in {
          default = pkgs.buildGoModule {
            name = "timewhisperer";
            pname = "timewhisperer";
            src = ./.;
            vendorHash = "sha256-ArYCbm+rj0VYQV58tiVyYPXGfgiW45hfc+wFGQuQy3U=";
            
            # Extract version directly from Git
            inherit version;
            buildInputs = with pkgs; [ go ];
            
            # Embed build information
            ldflags = [
              "-X" "main.Version=${version}"
              "-X" "main.GitCommit=${self.rev or "unknown"}"
              "-X" "main.BuildDate=1970-01-01T00:00:00Z" # Fixed timestamp for reproducibility
              "-s" "-w" # Strip debug symbols
            ];
            
            # Make sure build is hermetic
            env.CGO_ENABLED = "0";
            
            # Skip tests that expect binary in specific location
            doCheck = false;
            
            # Simplify output paths for better reproducibility
            preBuild = ''
              export GOCACHE=$TMPDIR/go-cache
              export GOPATH=$TMPDIR/go
            '';
            
            meta = with pkgs.lib; {
              description = "SneakTime - Upwork Screenshot Monitor";
              homepage = "https://github.com/yourusername/time-whisperer";
              license = licenses.mit;
              maintainers = [ ];
              platforms = platforms.all;
            };
          };
        }
      );
      
      # Add development shell for contributors
      devShells = forAllSystems (system:
        let pkgs = pkgsFor system;
        in {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              go
              gopls
              gotools
              go-tools
              nixpkgs-fmt
            ];
          };
        }
      );
    };
} 