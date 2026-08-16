<!-- @lede -->

Cinnabar targets LLVM 21 via `inkwell` and needs `clang`, `llc` and `opt` on
`PATH`, plus a static musl libc to link Cinnabar binaries against. The project
ships a Nix flake that provisions all of it.

<!-- @nix -->

The flake provides LLVM, clang, and the Rust toolchain. Static `--static`
builds on Linux self-provision musl from upstream at build time. Add
`--bin cinnabar-lsp` to build the language server as well.

<!-- @nix-outside -->

Outside `nix develop`, `cargo build` and `cargo clippy` will fail unless you have
a matching LLVM 21 toolchain and `clang` on `PATH`. `build.rs` self-provisions
musl for static builds (via `curl`/`wget`, `tar`, `make`, and `sha256sum`), so no
host musl package is required; `MUSL_LIBC_A` remains a manual override.

<!-- @first-program -->

`init` scaffolds `build.cnb`, `main.cnb` and `tests/smoke.cnb`. It refuses to
overwrite: if any of the three exists, it writes none of them.

<!-- @first-program-file -->

Or compile a single file straight through the pipeline:

<!-- @wsl -->

Windows contributors run the same flake under WSL2, with the checkout on the
distro's own filesystem. This is the default and needs no Docker: `flake.nix`
supplies the toolchain either way, and it is the shape CI runs the gate in — a
plain `ubuntu-latest` runner, no container anywhere.

Keeping the checkout under `/mnt/c` is the one thing to avoid. A Windows-hosted
checkout reaches Linux over a 9p bridge, and that bridge costs roughly 30–50× on
the small-file operations Cargo's fingerprint pass, Semgrep's walk and
rust-analyzer's watcher perform constantly. Git worktrees need no special
handling: `git worktree add` works directly, because the `gitdir:` pointer
problem the container workflow solves exists only when Windows Git writes a
`C:/...` path Linux cannot follow.

<!-- @docker -->

Where WSL2 is unavailable or unwanted, the same Nix environment runs in one
Docker Compose service instead, without installing Nix or LLVM on Windows. It
pays the 9p cost described above. Native Linux development is Nix-first and
needs none of this.

The Compose project has a single disposable `dev` service. Retargeting it
recreates the container, because Docker cannot change a running container's bind
mounts, so everything expensive lives in named volumes that survive the
recreation. The base image is `debian:bookworm-slim` pinned by digest with Nix
installed on top — Debian rather than a pure-Nix image because the VS Code
Server ships a prebuilt glibc-linked `node` and needs an FHS layout. `flake.nix`,
not the image, stays the source of truth for Rust, LLVM, Clang, Semgrep and the
rest.

<!-- @docker-configure -->

Run the helper from the repository root and give the worktree a stable,
lowercase cache key. This is the one script that runs on the *host* rather than
in the container — it writes the environment file every later `docker compose`
command reads, and there is no container to run it in yet. On Windows it needs
nothing beyond the Git Bash that Git for Windows installs; host paths are
converted with `cygpath -m` so `/c/...` reaches Compose as `C:/...`.

<!-- @docker-select -->

The helper writes ignored files under `container/local/<cache-key>/` and prints
the exact start and gate commands. Selecting a checkout after that takes only its
environment file — the generated file carries the Compose file selection itself,
so there are no `-f` arguments to remember or get wrong.

<!-- @docker-switch -->

To switch branches, confirm no command is running, then run the target
worktree's `up -d --build`. Compose recreates only the service: the Nix and Cargo
volumes stay shared, and the `target` volume changes with the cache key.

A linked Windows worktree needs one extra thing. Windows Git writes a worktree
pointer containing a host path such as `C:/path/to/cinnabar/.git/worktrees/...`,
which means nothing inside Linux, so the helper generates proxy pointer and
backlink files and `compose.worktree.yaml` mounts them. That override must not be
included for a main checkout, and is not left to the caller — the helper decides
from the checkout's `.git` and records the answer as `COMPOSE_FILE`.

<!-- @docker-volumes -->

What survives a recreation, and what does not:

<!-- @docker-safety -->

Rules the workflow depends on:

<!-- @docker-verify -->

For infrastructure changes, validate `config` for the main checkout and at least
two linked worktrees, passing no `-f` so the generated `COMPOSE_FILE` is what
gets exercised. A main checkout's output must contain no `/git-common` mount; a
linked worktree's must contain all four bind mounts. Then start each selection
and confirm:

<!-- @lsp -->

The repository also builds `cinnabar-lsp`, a Language Server Protocol server over
the same compiler pipeline. It speaks stdio and provides diagnostics — with the
borrow checker's explanatory notes as related information and code lenses —
hover, go-to-definition and find-references across the module graph, completion,
and signature help.

Every answer is read from the facts the pipeline attaches. The server contains no
second implementation of name resolution or type inference.

Point any LSP client at the binary for `.cnb` files. In Neovim:

<!-- @vscode-attach -->

Under WSL2, open the checkout with the **WSL** extension rather than Dev
Containers — `code --remote wsl+<distro> ~/dev/cinnabar`. The extension host then
runs inside the distro and the checked-in workspace settings apply unmodified.

Under the container, start the selected service, then run **Dev Containers:
Attach to Running Container…**, choose the `dev` container in the `cinnabar`
stack, and open `/workspace`.

Either way the terminal that opens is not an interactive Nix shell — run
`nix develop`, or prefix individual commands with `nix develop --command`.

<!-- @vscode-config -->

A name-level attached-container configuration keeps the editor setup across
service recreation. Save it as `nameConfigs/dev.json` under the Dev Containers
extension's global storage — on Windows,
`%APPDATA%/Code/User/globalStorage/ms-vscode-remote.remote-containers/` — or
create it from **Dev Containers: Open Attached Container Configuration File…**.
Keying on the name works because `compose.dev.yaml` pins `container_name: dev`.

The extension rewrites `extensions` to reflect what it has already installed, so
finding that array empty afterwards is expected. `settings` is the part that has
to persist.

<!-- @vscode-analyzer -->

`rust-analyzer.server.path` points at a wrapper that runs rust-analyzer through
`nix develop`, so the server and its build-script children see the same LLVM 21
and musl environment as a command-line build. Because of that wrapper, nothing in
`shellHook` may write to stdout: rust-analyzer speaks LSP there, and a banner
ahead of the first `Content-Length` header makes the editor discard the server.

To confirm the editor is driving the Nix toolchain rather than a bundled server,
check the process tree. Both the server and its `rust-analyzer-proc-macro-srv`
children must resolve to `/nix/store/...` paths, and the proc-macro server must
come from the same `rustc` that builds the crate — a mismatch is what produces
`mismatched ABI expected ... got ...`.

<!-- @vscode-extension -->

`editors/vscode` is not on the Marketplace, so no editor configuration can pull
it by identifier — it has to be built from the checkout and installed into
whichever VS Code Server you are running. The helper does that in either
environment; it needs a server and a `node`, not a container. Under the
container the extension lands in the `cinnabar-vscode-server` volume, so it
survives service recreation and worktree switches. Re-run it after changing the
extension, then reload the window.

`cinnabar.server.mode` ships as `path`, with `cinnabar.server.path` set to
`container/bin/cinnabar-lsp-nix`. A relative value resolves against the
repository root, so the setting is portable across checkouts. Compose mode
remains selectable but cannot be the default: it requires
`container/local/<cache-key>/worktree.env`, which is generated and ignored, so a
fresh clone does not have it. Selected explicitly, it means "use the
repository's development server", not "shell out to Docker" — an editor on the
Windows host runs the server through `docker compose exec`, while one attached
to the container runs the binary directly, because the service mounts no Docker
socket and ships no `docker` CLI.

The launcher prefers `container/bin/cinnabar-lsp-nix`, which rebuilds the server
and then execs it: Cargo replaces the binary on rebuild, but a running server
keeps its old image and goes on answering from the compiler as it was hours ago —
silently, because the process is healthy and only its answers are stale. A window
reload is therefore enough to pick up compiler changes. A failed build is not
fatal; the wrapper serves the previous binary so the editor stays usable while
the code is broken.

<!-- @gate -->

The repository's build gate is `pre_commit_check.sh`, run inside the Nix dev
shell. It runs `cargo check`, `cargo clippy -D warnings`, a custom Semgrep
ruleset, `cargo test`, CLI smoke checks, compiles and dumps several fixtures,
runs the compiled `spec.cnb` reference binary, and runs a battery of negative
fixtures.

<!-- @profiles -->

For quicker local feedback, the `balanced` and `smoke` profiles reduce the
randomized corpus sizes and the number of successful fixtures that are linked and
executed. Rejected fixtures remain checked in every profile.

Run the full gate with no profile override before submitting a change.

<!-- @outcome-title -->

A build either produces its artifact or produces diagnostics — never both, and
never part of one.

<!-- @outcome -->

On success the compiler prints
`Successfully compiled <input> to '<output>'.` and exits 0. Any failure is
rendered as source-located diagnostics and exits non-zero.
