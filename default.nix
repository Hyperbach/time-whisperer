# Compatibility layer for non-flake Nix users
(import (
  fetchTarball {
    url = "https://github.com/edolstra/flake-compat/archive/master.tar.gz";
    sha256 = "1prd9b1xx8c0sfwnyzksd2f4vbqn9hkg972k5sivq8wwrcn0pk7k";
  }
) {
  src = ./.;
}).defaultNix 