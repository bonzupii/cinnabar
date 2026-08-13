{
  description = "Cinnabar compiler development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            llvm
            clang
            libffi
            libxml2
            cargo
            rustc
            rustfmt
            clippy
            rust-analyzer
            pkg-config
            semgrep
            valgrind
            gdb
#            musl
            coq
          ];

          shellHook = ''
            # Banner goes to stderr, not stdout.  container/bin/rust-analyzer-nix
            # runs rust-analyzer through `nix develop`, and rust-analyzer speaks
            # LSP over stdout; anything printed there corrupts the stream before
            # the first Content-Length header and the editor drops the server.
            echo "LLVM $(llvm-config --version)" >&2
            echo "Rust $(rustc --version)" >&2
            export NIX_CFLAGS_COMPILE=""
            export NIX_HARDENING_ENABLE=""
            export RUST_BACKTRACE=full
            export MUSL_LIBC_A="${pkgs.musl}/lib/libc.a"
          '';
        };
      }
    );
}
