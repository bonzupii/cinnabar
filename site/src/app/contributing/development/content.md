The contributor environment keeps toolchain and build caches reusable while
each linked worktree retains its own checkout and target output.

<!-- @lede -->

Windows contributors can run the Nix environment in one Docker Compose service.
The repository helper selects the checkout, generates safe worktree mounts, and
assigns a stable cache key.

## Configure a checkout

```bash
./container/configure-worktree.sh --worktree "$PWD" --cache-key feature
docker compose --env-file "container/local/feature/worktree.env" up -d --build
```

The generated environment file records whether linked-worktree Git proxy mounts
are required. Do not hand-author the Compose file set.

## Reusable state

Nix, Cargo, and VS Code Server data live in named volumes. Rust build output is
keyed per worktree. Never share a target cache key between concurrently active
branches, and do not remove the reusable volumes during normal work.

## Attached editors

Attach VS Code to the running `dev` container and open `/workspace`. Run commands
inside `nix develop`; the supplied wrappers ensure rust-analyzer and
`cinnabar-lsp` use the selected checkout and current compiler binary.

## Verification gate

Run the repository gate once after a coherent code change:

```bash
nix develop --command ./pre_commit_check.sh
```

Its complete result is written to `pre_commit.log` in the selected checkout.
The repository’s `AGENTS.md` defines the current contribution constraints.
