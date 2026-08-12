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
| Gate log | `/workspace/pre_commit.log` | Written into the selected host checkout |

The service does not mount the Docker socket, the Windows user profile, or another worktree's source directory. Do not use `docker compose down --volumes` during normal work; that option deletes the reusable caches.

The base image is Nix 2.35.1 pinned by both tag and digest. `flake.nix`, not the image, remains the single source of truth for Rust, LLVM, Clang, Semgrep, Coq, rust-analyzer, and other development tools.

## Configure a checkout

Run the helper from the repository root in PowerShell. Give every worktree a stable, lowercase cache key:

```powershell
./container/configure-worktree.ps1 -Worktree $PWD -CacheKey main
```

The helper writes ignored files below `container/local/<cache-key>/` and prints exact start and gate commands. It never rewrites the checkout's `.git` data. For the main checkout, use only `compose.dev.yaml`:

```powershell
docker compose --env-file "container/local/main/worktree.env" -f compose.dev.yaml config
docker compose --env-file "container/local/main/worktree.env" -f compose.dev.yaml up -d --build
docker compose --env-file "container/local/main/worktree.env" -f compose.dev.yaml exec dev nix develop
docker compose --env-file "container/local/main/worktree.env" -f compose.dev.yaml exec dev nix develop --command ./pre_commit_check.sh
Get-Content pre_commit.log
```

For a linked Windows worktree, run the same helper from any checkout and pass its path:

```powershell
./container/configure-worktree.ps1 -Worktree "C:/path/to/cinnabar-feature" -CacheKey feature
```

Use both Compose files for the linked worktree, exactly as printed by the helper:

```powershell
docker compose --env-file "container/local/feature/worktree.env" -f compose.dev.yaml -f compose.worktree.yaml config
docker compose --env-file "container/local/feature/worktree.env" -f compose.dev.yaml -f compose.worktree.yaml up -d --build
docker compose --env-file "container/local/feature/worktree.env" -f compose.dev.yaml -f compose.worktree.yaml exec dev nix develop --command ./pre_commit_check.sh
```

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

The main checkout needs no proxy: its real `.git` directory arrives with the `/workspace` bind mount, so `compose.worktree.yaml` must not be included for it.

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

The wrapper runs rust-analyzer through `nix develop`, so it and its Cargo/build-script children see the same LLVM 21 and musl environment as command-line builds. After changing `flake.nix`, reload the VS Code window or restart rust-analyzer.

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

For infrastructure changes, validate `docker compose ... config` for the main checkout and at least two linked worktrees. Start each selection and confirm:

```bash
git status --short --branch
nix --version
nix develop --command rust-analyzer --version
nix develop --command ./pre_commit_check.sh
```

The exact gate must pass without profile overrides, and its `pre_commit.log` must appear only in the selected host worktree. Switching back to an earlier worktree must reuse its own target volume and the shared Nix/Cargo caches.

## References

- [Docker Compose volumes](https://docs.docker.com/reference/compose-file/volumes/)
- [Docker Compose service mounts](https://docs.docker.com/reference/compose-file/services/#volumes)
- [Docker Desktop WSL best practices](https://docs.docker.com/desktop/features/wsl/best-practices/)
- [VS Code: Attach to a running container](https://code.visualstudio.com/docs/devcontainers/attach-container)
- [rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html)
