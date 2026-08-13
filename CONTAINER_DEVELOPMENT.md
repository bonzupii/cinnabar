# Reusable Nix development container

Status: implemented. This workflow gives Windows-hosted checkouts the repository's Nix toolchain without installing LLVM or Nix on Windows. Native Linux development remains Nix-first:

```bash
nix develop
nix develop --command ./pre_commit_check.sh
```

## What is shared and what is isolated

The Compose project has one disposable `dev` service. Retargeting it recreates the container because Docker cannot change a running container's bind mounts. The expensive data survives in explicitly named volumes:

| Data | Container location | Lifetime |
|---|---|---|
| Selected checkout | `/workspace` | Host bind mount; one checkout at a time |
| Nix store/database | `/nix` | Shared `cinnabar-nix` volume |
| Nix flake fetch cache | `/root/.cache/nix` | Shared `cinnabar-nix-cache` volume |
| Cargo home | `/root/.cargo` | Shared `cinnabar-cargo` volume |
| Rust build output | `/workspace/target` | `cinnabar-target-<cache-key>`, isolated per worktree |
| VS Code Server and its extensions | `/root/.vscode-server` | Shared `cinnabar-vscode-server` volume |
| Gate log | `/workspace/pre_commit.log` | Written into the selected host checkout |

The service does not mount the Docker socket, the Windows user profile, or another worktree's source directory. Do not use `docker compose down --volumes` during normal work; that option deletes the reusable caches.

The base image is `debian:bookworm-slim`, pinned by digest, with Nix 2.35.1 installed on top. `flake.nix`, not the image, remains the single source of truth for Rust, LLVM, Clang, Semgrep, Coq, rust-analyzer, and other development tools.

Debian rather than `nixos/nix` for one reason: the VS Code Server ships a prebuilt glibc-linked `node` binary and needs an FHS layout — the loader at `/lib64/ld-linux-x86-64.so.2`, `libstdc++`, `libc`, and `ldconfig`. A pure-Nix image has none of them, so **Attach to Running Container** fails its requirements check and the server never starts. Because Compose mounts the shared `cinnabar-nix` volume over `/nix`, the Nix installed into the image only seeds a *fresh* volume; an existing store is used as-is. Both layouts expose `/nix/var/nix/profiles/default/bin`, which is what `PATH` resolves against.

## Configure a checkout

Run the helper from the repository root. Give every worktree a stable, lowercase cache key:

```bash
./container/configure-worktree.sh --worktree "$PWD" --cache-key main
```

On Windows this is the Git Bash that Git for Windows already installs; the helper needs nothing beyond bash and Git. It is the one script here that must run on the *host* rather than in the container, because it produces the environment file every later `docker compose --env-file` command reads — there is no container to run it in yet. Host paths reach Compose through `cygpath -m`, so `/c/...` becomes the `C:/...` form Docker accepts as a bind source; on a Unix host there is nothing to convert.

The helper writes ignored files below `container/local/<cache-key>/` and prints exact start and gate commands. It never rewrites the checkout's `.git` data. Selecting a checkout takes only its environment file:

```bash
docker compose --env-file "container/local/main/worktree.env" config
docker compose --env-file "container/local/main/worktree.env" up -d --build
docker compose --env-file "container/local/main/worktree.env" exec dev nix develop
docker compose --env-file "container/local/main/worktree.env" exec dev nix develop --command ./pre_commit_check.sh
cat pre_commit.log
```

Both arguments have defaults: `--worktree` is the current directory and `--cache-key` is that directory's name. For a linked Windows worktree, run the same helper from any checkout and pass its path:

```bash
./container/configure-worktree.sh --worktree /c/path/to/cinnabar-feature --cache-key feature
```

The commands for it are identical apart from the cache key — there are no `-f` arguments to remember or get wrong:

```bash
docker compose --env-file "container/local/feature/worktree.env" config
docker compose --env-file "container/local/feature/worktree.env" up -d --build
docker compose --env-file "container/local/feature/worktree.env" exec dev nix develop --command ./pre_commit_check.sh
```

That works because the generated file carries the Compose file selection itself:

```text
COMPOSE_PATH_SEPARATOR=;
COMPOSE_FILE=compose.dev.yaml;compose.worktree.yaml
```

Which Compose files apply was the only thing that differed between a main checkout and a linked worktree, and it is derived from the same `.git` inspection that produces the rest of the file — so it travels with the selection rather than being retyped at each call site. A main checkout gets `COMPOSE_FILE=compose.dev.yaml` alone. An explicit `-f` still overrides the variable, so older invocations keep working.

`COMPOSE_PATH_SEPARATOR` is pinned rather than left to the platform default (`;` on Windows, `:` elsewhere) so one generated file means the same thing on either host; `;` rather than `:` because a drive letter contains a colon.

Paths in the printed commands are repository-relative because Compose resolves `COMPOSE_FILE` against the working directory too: both assume the repository root, and a relative path stays valid whether you paste it into bash or PowerShell.

To switch branches, first confirm no command is running, then run the target worktree's `up -d --build` command. Compose recreates only the service. The Nix and Cargo volumes remain shared and the target volume changes with the cache key.

## Why linked Windows worktrees need an override

Windows Git writes a linked worktree pointer containing a host path such as:

```text
gitdir: C:/path/to/cinnabar/.git/worktrees/cinnabar-feature
```

That path is meaningless inside Linux. The helper creates two small proxy files and `compose.worktree.yaml` mounts this proven topology:

```text
HOST linked worktree                 -> /workspace
HOST main checkout .git directory    -> /git-common
HOST generated pointer file          -> /workspace/.git
HOST generated backlink file         -> /git-common/worktrees/<admin>/gitdir
```

Inside the container, the proxy pointer says `gitdir: /git-common/worktrees/<admin>` and the proxy backlink says `/workspace/.git`. This preserves Git and Nix flake discovery without modifying host metadata or mounting a second alias of the source tree. Avoiding that duplicate source alias also prevents repository scanners from visiting the same files twice.

The main checkout needs no proxy: its real `.git` directory arrives with the `/workspace` bind mount, so `compose.worktree.yaml` must not be included for it. The helper decides that from the checkout's `.git` and records it as `COMPOSE_FILE`, so including the override for a main checkout is not a mistake left to the caller.

## VS Code

Start the selected service, then use **Dev Containers: Attach to Running Container...**, choose the `dev` container in the `cinnabar` stack, and open `/workspace`. A name-level attached-container configuration can keep the editor setup across service recreation:

```json
{
  "workspaceFolder": "/workspace",
  "extensions": [
    "rust-lang.rust-analyzer"
  ],
  "settings": {
    "terminal.integrated.defaultProfile.linux": "bash",
    "rust-analyzer.server.path": "/workspace/container/bin/rust-analyzer-nix"
  },
  "remoteUser": "root"
}
```

Save that JSON as `nameConfigs/dev.json` under the Dev Containers extension's global storage (`%APPDATA%/Code/User/globalStorage/ms-vscode-remote.remote-containers/` on Windows), or create it from **Dev Containers: Open Attached Container Configuration File...**. Keying it on the name works because `compose.dev.yaml` pins `container_name: dev`, so the config survives service recreation. The extension rewrites `extensions` to reflect what it has already installed, so finding that array empty afterwards is expected; `settings` is the part that must persist.

To confirm the editor is genuinely driving the Nix toolchain rather than a bundled server, check the process tree inside the container:

```bash
docker exec dev ps -eo pid,ppid,args | grep -E "rust-analyzer" | grep -v grep
```

Both the server and its `rust-analyzer-proc-macro-srv` children must resolve to `/nix/store/...` paths, and the proc-macro server must come from the same `rustc-<version>` that builds the crate. A proc-macro server from a different rustc is the cause of `mismatched ABI expected ... got ...` errors.

The wrapper runs rust-analyzer through `nix develop`, so it and its Cargo/build-script children see the same LLVM 21 and musl environment as command-line builds. After changing `flake.nix`, reload the VS Code window or restart rust-analyzer.

Because of that wrapper, **nothing in `shellHook` may write to stdout**. rust-analyzer speaks LSP over stdout, so a hook that echoes a banner there emits it ahead of the first `Content-Length` header and the editor discards the server. Send diagnostic output to stderr (`echo ... >&2`), as `flake.nix` does. To check the stream is clean:

```bash
docker compose --env-file "container/local/main/worktree.env" \
  exec dev sh -c 'cd /workspace && container/bin/rust-analyzer-nix --version 2>/dev/null'
```

The only line on stdout must be rust-analyzer's own version.

### The Cinnabar extension

`editors/vscode` is not on the Marketplace, so an attached-container configuration cannot pull it by identifier. Package and install it into the container once:

```bash
docker compose --env-file "container/local/main/worktree.env" \
  exec dev nix develop --command ./container/install-vscode-extension.sh
```

The helper runs inside the container, like `pre_commit_check.sh`, and takes its `node` from the dev shell. Nothing but Docker is required on the host, so the command above is identical from PowerShell, bash, and zsh. Running it from a shell already inside the container works too; run it anywhere else and it prints the invocation above rather than half-working.

Every helper in this repository is a single `.sh`, deliberately: a shell copy and a PowerShell copy of the same logic drift apart. Only `configure-worktree.sh` runs on the host, and it stays portable by converting paths rather than by being rewritten per shell.

The extension lands in the `cinnabar-vscode-server` volume alongside the server itself, so it survives service recreation and worktree switches. Re-run the helper after changing the extension, then reload the window.

`cinnabar.server.mode` stays `docker-compose` in `.vscode/settings.json` and means "use the repository's development server" rather than "shell out to Docker". Which of those it does depends on where the editor runs:

| Editor location | Launch |
|---|---|
| Windows host | `docker compose ... exec -T dev ./target/debug/cinnabar-lsp` |
| Attached to the container | `/workspace/target/debug/cinnabar-lsp` directly |

The second case is not an optimization. This service mounts no Docker socket and ships no `docker` CLI, so an attached editor that took the Compose path could never start the server. `compose.dev.yaml` sets `CINNABAR_IN_DEV_CONTAINER=1` and `editors/vscode/lsp-launcher.js` branches on it; an explicit marker is used rather than sniffing for `/.dockerenv`, which also matches containers that cannot serve this repository.

The launcher prefers `container/bin/cinnabar-lsp-nix`, which rebuilds the server and then execs it. Cargo replaces the binary on rebuild but a running server keeps its old image, so an editor started before a rebuild goes on answering from the compiler as it was hours ago — silently, because the process is healthy and only its answers are stale. Building at every server start ties the server to the source; a window reload is therefore enough to pick up compiler changes, and nothing else needs remembering.

To confirm the running server is not stale:

```bash
docker exec dev sh -c 'readlink /proc/$(pgrep -f "cinnabar[-]lsp --stdio" | head -1)/exe'
```

A path ending in `(deleted)` means the binary was replaced underneath a running server: reload the window.

A failed build is not fatal — the wrapper serves the previous binary so the editor stays usable while the code is broken. Build output goes to stderr, because stdout is the LSP stream. On a fresh target volume with no binary at all, build it once:

```bash
nix develop --command cargo build --bin cinnabar-lsp
```

To confirm the whole chain after opening a `.cnb` file:

```bash
docker exec dev ps -eo args | grep -E "cinnabar-lsp|bin/rust-analyzer$"
```

An attached terminal starts in `/workspace` but is not itself an interactive Nix shell. Either run `nix develop` or prefix individual commands:

```bash
nix develop --command cargo check
nix develop --command cargo test
nix develop --command ./pre_commit_check.sh
```

Branch switching and worktree creation/removal remain host responsibilities. Close the attached editor before retargeting the service, then reattach and confirm `/workspace` contains the expected branch.

## Performance and safety

- Allocate at least 8 GiB to Docker Desktop; 12 GiB is preferable for the full gate.
- High CPU allocation is useful. Do not bake emergency limits such as `CARGO_BUILD_JOBS=2` or `CARGO_PROFILE_DEV_DEBUG=0` into the workflow.
- Source stays bind-mounted so edits and `pre_commit.log` are visible on Windows. Nix, Cargo, and `target` stay in Linux named volumes for speed.
- Never share a target cache key between concurrently active worktrees.
- Never run two branches through this single service at once.
- Never rewrite a host `.git` pointer to a Linux path.
- Always inspect `pre_commit.log` from the selected host checkout after the gate exits.

## Verification checklist

For infrastructure changes, validate `docker compose --env-file "container/local/<cache-key>/worktree.env" config` for the main checkout and at least two linked worktrees, passing no `-f` so the generated `COMPOSE_FILE` is what gets exercised. The main checkout's output must contain no `/git-common` mount and a linked worktree's must contain all four bind mounts from the topology above. Start each selection and confirm:

```bash
git status --short --branch
nix --version
nix develop --command rust-analyzer --version
nix develop --command ./pre_commit_check.sh
```

The exact gate must pass without profile overrides, and its `pre_commit.log` must appear only in the selected host worktree. Switching back to an earlier worktree must reuse its own target volume and the shared Nix/Cargo caches.

## References

- [Docker Compose pre-defined environment variables](https://docs.docker.com/compose/how-tos/environment-variables/envvars/) (`COMPOSE_FILE`, `COMPOSE_PATH_SEPARATOR`)
- [Docker Compose volumes](https://docs.docker.com/reference/compose-file/volumes/)
- [Docker Compose service mounts](https://docs.docker.com/reference/compose-file/services/#volumes)
- [Docker Desktop WSL best practices](https://docs.docker.com/desktop/features/wsl/best-practices/)
- [VS Code: Attach to a running container](https://code.visualstudio.com/docs/devcontainers/attach-container)
- [rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html)
