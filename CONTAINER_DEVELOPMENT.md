# Reusable development container plan

Status: proposed workflow. This document is intentionally implementation-ready, but it does not add or replace the repository's native Nix workflow.

## Goals

- Download the Nix and Rust toolchains once and reuse them across branches and linked worktrees.
- Keep each worktree's source tree, generated `pre_commit.log`, and build artifacts isolated.
- Run the repository gate exactly as required:

  ```bash
  nix develop --command ./pre_commit_check.sh
  ```

- Work from Windows-hosted checkouts without requiring LLVM, Clang, or Nix on the Windows host.
- Keep native Linux development simple: Linux contributors should normally use `nix develop` directly and should not need Docker.
- Never modify `pre_commit_check.sh`, inspect the Semgrep configuration, or make the container depend on a fixture-specific shortcut.

## Decision

Use one long-lived Compose service named `dev`, backed by a pinned image and explicitly named cache volumes. Treat the container itself as disposable: changing the selected worktree recreates the single `dev` container, while the expensive image and cache volumes remain in place.

This distinction matters because Docker cannot change a running container's bind mounts. Preserving a particular container ID would make worktree selection brittle. Recreating one small service around durable volumes is the normal Compose lifecycle and avoids downloading the toolchain again.

The default workflow permits one verification gate at a time. It must not run two branches concurrently through the same container or the same `target` volume. Parallel verification can be added later with isolated service instances and target volumes, but it is not the default because it makes source ownership and results harder to reason about.

## Proposed files

The implementation should add these files in one focused change:

```text
compose.dev.yaml                 Compose service and named-volume declarations
container/Containerfile         Pinned Nix base image and stable container defaults
container/bin/rust-analyzer-nix Launch rust-analyzer through the Nix dev shell
container/worktree.env.example  Documented, non-secret per-worktree settings
.dockerignore                   Small image build context; excludes target and Git data
CONTAINER_DEVELOPMENT.md        This workflow and its acceptance criteria
```

Do not put a real developer path in a committed environment file. A local `container/worktree.env` should be ignored and selected explicitly with `--env-file`.

## Cache and mount model

| Data | Mount | Lifetime and sharing rule |
|---|---|---|
| Selected worktree | bind mount at `/workspace` | Read/write; exactly one branch per container |
| Common Git metadata | bind mount at the path encoded by the worktree's `.git` pointer | Read/write because Git/Nix may take locks; never used to change another checkout |
| Selected worktree alias | second bind of the selected worktree at its encoded Windows path beneath `/workspace` | Required for Windows `core.worktree` resolution |
| Nix store and database | named volume at `/nix` | Shared across sequential worktree selections; this is the main toolchain cache |
| Cargo registry | named volume at `/root/.cargo/registry` | Shared across sequential worktree selections |
| Cargo Git checkouts | named volume at `/root/.cargo/git` | Shared across sequential worktree selections |
| Rust build output | named volume at `/workspace/target` | One explicit volume name per worktree/cache key; never shared concurrently across branches |
| `pre_commit.log` | ordinary file in `/workspace` | Remains visible in the host worktree and identifies which branch produced the result |

Use explicit top-level volume `name` values, such as `cinnabar-nix`, rather than Compose's project-scoped defaults. Otherwise running the Compose file from a differently named worktree silently creates another multi-gigabyte cache.

Do not mount the Docker socket, use privileged mode, or mount the entire Windows user profile. The source and Git mounts above are the only host filesystem access the service needs.

## Why Windows worktrees need three source/Git mounts

A linked worktree created by Windows Git has a `.git` file similar to:

```text
gitdir: C:/Users/Lawrence/Documents/Dev/cinnabar/.git/worktrees/cinnabar-tooling-followup
```

The common Git configuration can also record the worktree as a Windows absolute path. A Linux container does not automatically resolve either path. Mounting only the selected worktree at `/workspace` therefore makes Nix's Git-backed flake lookup fail before the development shell starts.

The proven topology is:

```text
HOST selected worktree
  -> /workspace

HOST main checkout/.git
  -> /workspace/C:/Users/Lawrence/Documents/Dev/cinnabar/.git

HOST selected worktree
  -> /workspace/C:/Users/Lawrence/Documents/Dev/<selected-worktree>
```

The two encoded targets are deliberately under `/workspace`. On Linux, the Windows `C:/...` spelling in the `.git` file is interpreted relative to the selected worktree, so these aliases make both the Git common directory and `core.worktree` resolve without rewriting host Git metadata.

The Compose implementation should parameterize all host sources and encoded targets. It must not hardcode `cinnabar-tooling-followup`, another branch name, or one developer's home directory.

Suggested local configuration:

```dotenv
CINNABAR_WORKTREE=C:/Users/Lawrence/Documents/Dev/cinnabar-tooling-followup
CINNABAR_GIT_COMMON=C:/Users/Lawrence/Documents/Dev/cinnabar/.git
CINNABAR_GIT_COMMON_POINTER=C:/Users/Lawrence/Documents/Dev/cinnabar/.git
CINNABAR_WORKTREE_POINTER=C:/Users/Lawrence/Documents/Dev/cinnabar-tooling-followup
CINNABAR_CACHE_KEY=tooling-followup
```

`CINNABAR_CACHE_KEY` must contain only a short, filesystem-safe identifier. It chooses a target volume such as `cinnabar-target-tooling-followup`; it does not determine Git behavior.

## Compose shape

The implementation should follow this structure. The image reference must be replaced with a tested, pinned Nix version and digest rather than leaving a floating `latest` tag.

```yaml
name: cinnabar-dev

services:
  dev:
    build:
      context: .
      dockerfile: container/Containerfile
    init: true
    working_dir: /workspace
    command: ["sleep", "infinity"]
    environment:
      NIX_CONFIG: "experimental-features = nix-command flakes"
      RUST_BACKTRACE: "full"
    volumes:
      - type: bind
        source: ${CINNABAR_WORKTREE:?set CINNABAR_WORKTREE}
        target: /workspace
      - type: bind
        source: ${CINNABAR_GIT_COMMON:?set CINNABAR_GIT_COMMON}
        target: /workspace/${CINNABAR_GIT_COMMON_POINTER:?set CINNABAR_GIT_COMMON_POINTER}
      - type: bind
        source: ${CINNABAR_WORKTREE:?set CINNABAR_WORKTREE}
        target: /workspace/${CINNABAR_WORKTREE_POINTER:?set CINNABAR_WORKTREE_POINTER}
      - type: volume
        source: nix-store
        target: /nix
      - type: volume
        source: cargo-registry
        target: /root/.cargo/registry
      - type: volume
        source: cargo-git
        target: /root/.cargo/git
      - type: volume
        source: rust-target
        target: /workspace/target

volumes:
  nix-store:
    name: cinnabar-nix
  cargo-registry:
    name: cinnabar-cargo-registry
  cargo-git:
    name: cinnabar-cargo-git
  rust-target:
    name: cinnabar-target-${CINNABAR_CACHE_KEY:?set CINNABAR_CACHE_KEY}
```

Before adopting this verbatim, validate the interpolated Windows targets with:

```powershell
docker compose --env-file container/worktree.env -f compose.dev.yaml config
```

The rendered configuration must show three distinct source/Git bind mounts and must not expose unrelated host directories.

## Day-to-day commands on Windows

Create the local configuration once for each worktree by copying the example and filling in its paths. Then start or retarget the reusable service:

```powershell
docker compose --env-file container/worktree.env -f compose.dev.yaml up -d --build
```

Enter the Nix development shell:

```powershell
docker compose --env-file container/worktree.env -f compose.dev.yaml exec dev nix develop
```

Run the required gate, unchanged:

```powershell
docker compose --env-file container/worktree.env -f compose.dev.yaml exec dev nix develop --command ./pre_commit_check.sh
Get-Content pre_commit.log
```

To switch the one service to another worktree:

1. Confirm no gate is running.
2. Update `container/worktree.env` with the new worktree, pointer paths, and cache key.
3. Recreate only the service:

   ```powershell
   docker compose --env-file container/worktree.env -f compose.dev.yaml up -d --force-recreate
   ```

The Nix and Cargo named volumes remain. The new worktree gets its own `target` volume. Do not add `--volumes` when stopping or recreating the service; that option intentionally deletes the caches this workflow is designed to retain.

## Using VS Code on Windows

Use VS Code's **Dev Containers** extension to attach to the Compose service after it has been started from PowerShell. Do not ask VS Code to generate a second Dockerfile or Compose project: the repository's `compose.dev.yaml` remains the owner of mounts, cache names, and worktree selection.

### First connection

1. Install [Visual Studio Code](https://code.visualstudio.com/) and Microsoft's [Dev Containers extension](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) on Windows.
2. Start the selected worktree's service from a host PowerShell terminal:

   ```powershell
   docker compose --env-file container/worktree.env -f compose.dev.yaml up -d --build
   ```

3. In VS Code, open the Command Palette and select **Dev Containers: Attach to Running Container...**.
4. Select the container whose Compose project is `cinnabar-dev` and whose service is `dev`.
5. Select **File: Open Folder...** and open `/workspace`.

VS Code installs remote extensions into the Linux container. The editor window should show a Dev Container indicator in the lower-left corner, and its integrated terminal should report `/workspace` as the current directory. Files still live in the selected host worktree; saving in VS Code updates the Windows files through the bind mount.

### Persistent attached-container settings

After the first connection, run **Dev Containers: Open Named Configuration File...**. A name-level configuration survives recreation of the service and can reopen the correct folder and reinstall the required extension:

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

This is local VS Code state, not a repository file. Keeping it name-scoped prevents the configuration from accidentally applying to unrelated Nix containers that use the same base image.

### Make rust-analyzer use the flake

Attaching VS Code to the container does not by itself place extension processes inside `nix develop`. If rust-analyzer starts with the container's base environment, its Cargo subprocesses can fail to find LLVM 21, `llvm-config`, Clang, or the staged musl libc even though terminal builds work.

The implementation should therefore add `rust-analyzer` to `flake.nix` and provide this small executable wrapper at `container/bin/rust-analyzer-nix`:

```sh
#!/usr/bin/env sh
exec nix develop --command rust-analyzer "$@"
```

The VS Code setting above points the extension at the wrapper. The language server and every Cargo, rustc, build-script, and rustfmt process it launches then inherit the same development-shell environment as command-line builds. This preserves the single-source-of-truth rule: no generated list of Nix store paths or duplicated LLVM environment variables is checked into `.vscode/settings.json`.

After changing `flake.nix` or the wrapper, run **Developer: Reload Window** or **Rust Analyzer: Restart server**. Verify the setup from **View: Output → Rust Analyzer Language Server**; it should no longer contain "No suitable version of LLVM was found".

Until that wrapper is implemented, VS Code remains safe for editing and container terminals, but host-like rust-analyzer diagnostics are not authoritative. Use the exact Nix verification command for results.

### Integrated terminal and tasks

An attached terminal is inside the container but is not automatically an interactive Nix development shell. Use either:

```bash
nix develop
```

for an interactive shell, or prefix individual operations explicitly:

```bash
nix develop --command cargo check
nix develop --command cargo test
nix develop --command ./pre_commit_check.sh
```

If a VS Code task is added later, its command must be the exact gate rather than a second hand-maintained approximation:

```json
{
  "label": "Cinnabar: full verification gate",
  "type": "process",
  "command": "nix",
  "args": [
    "develop",
    "--command",
    "./pre_commit_check.sh"
  ],
  "options": {
    "cwd": "${workspaceFolder}"
  },
  "problemMatcher": []
}
```

The task must not invoke host PowerShell, host Cargo, or a shortened list of checks.

### Switching VS Code to another worktree

1. Stop any running terminal command or verification gate.
2. Close the attached VS Code window.
3. Update `container/worktree.env` and recreate the Compose service as described above.
4. Attach again and confirm that `/workspace` contains the expected branch before editing.

The named attached-container configuration will reinstall rust-analyzer if the recreated container does not retain the VS Code server state. The worktree-specific target volume follows `CINNABAR_CACHE_KEY`, so rust-analyzer and manual builds cannot silently reuse another branch's `target` directory.

VS Code's Source Control view may be used for status, diffs, and local edits once the three-mount Git topology is active. Branch switching, worktree creation/removal, and other checkout lifecycle operations remain host responsibilities. Do not use the attached container to change another task's branch or worktree, and do not mount the Docker socket merely to make VS Code manage the already-running service from inside itself.

### Optional Cinnabar language-server development

To exercise `cinnabar-lsp` from the attached editor, build it inside Nix:

```bash
nix develop --command cargo build --bin cinnabar-lsp
```

The resulting server is `/workspace/target/debug/cinnabar-lsp` in that worktree's target volume. A generic VS Code LSP client may be pointed at that path for `.cnb` files. Its configuration must remain worktree-local until the repository adopts an official Cinnabar VS Code extension; do not commit editor settings that assume one developer's container or target path.

## Native Linux workflow

The primary Linux path remains:

```bash
nix develop
nix develop --command ./pre_commit_check.sh
```

No Compose file should be required for a Linux contributor who already has Nix. The flake remains the single source of truth for LLVM, Clang, Rust, musl, Semgrep, and the other development tools. The container only provides a portable host for that flake.

A Linux contributor who chooses Docker may use the same service, but the implementation must document their actual Git pointer paths rather than assuming the Windows `C:/...` topology.

## Performance policy

- Allocate at least 8 GiB of memory to Docker Desktop for the full gate; 12 GiB is preferable when the host has room. CPU allocation can remain high.
- Do not bake low-memory emergency values such as `CARGO_BUILD_JOBS=2` or `CARGO_PROFILE_DEV_DEBUG=0` into the default workflow. They are optional local overrides, not project semantics.
- Keep `/nix` and Cargo caches in named Linux volumes. Docker documents named volumes as the persistent store for container-managed data, and Docker Desktop recommends volumes rather than Windows bind mounts for non-source data.
- Keep source as a bind mount so edits and `pre_commit.log` remain visible on the host.
- Keep `target` out of the Windows bind mount. It contains many small files and platform-specific build products; a named Linux volume is faster and avoids cross-branch contamination.
- If the repository is later moved into the WSL filesystem, bind-mount performance will improve. That is an optional migration, not a prerequisite for this workflow.

## Container image policy

The `Containerfile` should be intentionally small:

1. Start from a tested `nixos/nix` release pinned by tag and digest.
2. Enable `nix-command` and flakes.
3. Add only utilities required to keep the container alive or support the flake bootstrap.
4. Do not duplicate LLVM, Rust, rust-analyzer, Semgrep, musl, or Coq installation in the image; `flake.nix` owns those versions.
5. Prime the flake only as an optional image-build optimization. Correctness must not depend on a prewarmed store, and `flake.lock` changes must invalidate any priming layer.

Using a floating base tag makes an old branch unexpectedly acquire a different Nix implementation. The selected tag and digest should be updated deliberately and verified through the full gate.

## Worktree safety rules

- A container has one selected worktree. Never mount one branch over another branch's `/workspace` while a command is running.
- Never share a `target` volume between concurrently active worktrees.
- Do not run Git branch-changing commands inside the verification container. Checkout and worktree lifecycle remain host responsibilities.
- Do not rewrite a worktree's host `.git` pointer to a Linux path. Use the alias mounts instead.
- Do not copy source between worktrees to make a gate pass.
- Always read `pre_commit.log` from the selected host worktree after the gate exits.
- Stopping or recreating the service is safe. Deleting the named volumes is an explicit cache-reset operation and is outside the normal workflow.

## Implementation sequence

1. Pin and build the minimal container image.
2. Add the parameterized Compose service and explicit volume names.
3. Add the ignored local environment file pattern and committed example.
4. Validate `docker compose config` for the main checkout and two linked Windows worktrees.
5. Start the service for worktree A and run the exact gate.
6. Recreate the service for worktree B, confirm that no Nix toolchain download repeats, and run the exact gate.
7. Switch back to worktree A and confirm its own `target` cache and `pre_commit.log` remain isolated.
8. Attach VS Code to each selected worktree, confirm `/workspace` changes with the Compose selection, and verify rust-analyzer starts through the Nix wrapper without LLVM discovery errors.
9. Validate the native Linux commands independently; Docker success is not evidence that the native flake path still works.
10. Add a short link from `README.md` only after concurrent README work has landed, to avoid merging unrelated documentation edits.

## Acceptance criteria

The workflow is complete only when all of the following are demonstrated:

- A clean Docker Desktop machine can build the pinned image and populate the named caches.
- The main checkout and at least two Windows linked worktrees can each enter `nix develop` without editing their `.git` files.
- Switching worktrees recreates only the service; the Nix and Cargo caches survive.
- `nix develop --command ./pre_commit_check.sh` passes from each selected worktree.
- The gate writes `pre_commit.log` into the correct host worktree.
- Each worktree uses a distinct target volume.
- VS Code can attach to the running `dev` service, reopen `/workspace`, and reinstall its remote rust-analyzer extension after service recreation.
- Rust-analyzer and its Cargo/build-script children run inside the flake environment rather than a copied approximation of its environment variables.
- A failed build cannot be mistaken for a test result from a stale binary; the existing gate remains unchanged.
- No Compose mount exposes the Docker socket, the whole user profile, or another task's source tree.
- Native Linux `nix develop` and the exact verification command continue to work without Docker.

## References

- [Docker Compose volumes](https://docs.docker.com/reference/compose-file/volumes/)
- [Docker Compose service mounts](https://docs.docker.com/reference/compose-file/services/#volumes)
- [Docker Desktop WSL best practices](https://docs.docker.com/desktop/features/wsl/best-practices/)
- [Docker Desktop resource and file-sharing settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/)
- [Docker Compose trust model](https://docs.docker.com/compose/trust-model/)
- [VS Code: Attach to a running container](https://code.visualstudio.com/docs/devcontainers/attach-container)
- [VS Code: Use an existing Docker Compose service](https://code.visualstudio.com/docs/devcontainers/create-dev-container#_use-docker-compose)
- [rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html)
