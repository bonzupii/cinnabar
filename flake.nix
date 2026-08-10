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
            pkg-config
            semgrep
            valgrind
            gdb
#            musl
            coq
          ];

          shellHook = ''
            echo "LLVM $(llvm-config --version)"
            echo "Rust $(rustc --version)"
            export NIX_CFLAGS_COMPILE=""
            export NIX_HARDENING_ENABLE=""
            export RUST_BACKTRACE=full
            export MUSL_LIBC_A="${pkgs.musl}/lib/libc.a"
          '';
        };
      }
    );
}
