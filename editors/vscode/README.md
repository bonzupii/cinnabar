# Cinnabar Language Support

This extension registers `.cnb` files, provides the canonical TextMate grammar, and launches
`cinnabar-lsp` over stdio.

## Language-server launch modes

`cinnabar.server.mode` controls the process the extension starts:

- `auto` (default): launches `cinnabar.server.path` when that setting is non-empty; otherwise
  launches the installed `cinnabar-lsp` command from `PATH`.
- `installed`: always launches `cinnabar-lsp` from `PATH`.
- `path`: launches the executable configured by `cinnabar.server.path` and reports an error if it
  is empty.
- `docker-compose`: finds the nearest workspace ancestor containing `compose.dev.yaml` and
  `container/local/main/worktree.env`, sets that directory as the child process working directory,
  and launches:

  ```text
  docker compose --env-file container/local/main/worktree.env -f compose.dev.yaml exec -T dev ./target/debug/cinnabar-lsp
  ```

The repository workspace uses `docker-compose` mode in `.vscode/settings.json`, so it contains no
machine-specific executable or temporary wrapper. Before using that mode, start the development
container and ensure `container/local/main/worktree.env` identifies the checkout as required by
the Compose file.

For local development, run `npm install` in this directory and launch the extension through the
VS Code Extension Development Host. Run `npm test` to exercise launch-plan selection and
`npm run package:dry-run` to inspect the extension package contents.
