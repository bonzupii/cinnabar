#!/usr/bin/env bash
# Packages editors/vscode and installs it into this container's VS Code
# Server, so an attached editor starts the Cinnabar language server with no
# manual steps.  The extension lands in the cinnabar-vscode-server volume and
# survives service recreation; re-run this after changing the extension.
#
# Runs *inside* the dev container, like pre_commit_check.sh, so it needs
# nothing from the host but Docker.  That keeps one script for every host OS
# rather than a shell copy and a PowerShell copy drifting apart.
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXTENSION_ROOT="${REPOSITORY_ROOT}/editors/vscode"
VSIX="${TMPDIR:-/tmp}/cinnabar-language-support.vsix"

if [ "${CINNABAR_IN_DEV_CONTAINER:-}" = "" ]; then
  echo "This runs inside the dev container.  From the repository root:" >&2
  echo "" >&2
  echo "  docker compose --env-file container/local/main/worktree.env \\" >&2
  echo "    exec dev nix develop --command ./container/install-vscode-extension.sh" >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "node is not on PATH; run this through 'nix develop --command'." >&2
  exit 1
fi

if [ ! -f "${EXTENSION_ROOT}/package.json" ]; then
  echo "No extension manifest under ${EXTENSION_ROOT}" >&2
  exit 1
fi

# The extension requires vscode-languageclient at runtime, so the package must
# carry node_modules; without them activation fails with MODULE_NOT_FOUND.
if [ ! -d "${EXTENSION_ROOT}/node_modules" ]; then
  echo "Installing extension dependencies..."
  (cd "${EXTENSION_ROOT}" && npm install --no-audit --no-fund)
fi

echo "Packaging extension..."
# vsce stays an npx fetch rather than a devDependency: package.json lists
# node_modules in "files", so anything installed here ships inside the vsix.
(cd "${EXTENSION_ROOT}" && npx --yes @vscode/vsce package --allow-missing-repository --out "${VSIX}")

# The server directory is named for the VS Code commit, which changes whenever
# the editor updates, so discover it rather than pinning it.
SERVER_CLI="$(ls -dt "${HOME}"/.vscode-server/bin/*/bin/code-server 2>/dev/null | head -1 || true)"
if [ -z "${SERVER_CLI}" ]; then
  echo "No VS Code Server in this container.  Attach the editor once so it" >&2
  echo "installs itself, then re-run this script." >&2
  exit 1
fi

# --force because the version in package.json rarely changes between local
# rebuilds: without it the CLI sees the same version already installed, skips
# the install, and still exits 0 -- so the developer reloads the window and
# keeps running the extension they just replaced.
"${SERVER_CLI}" --install-extension "${VSIX}" --force \
  --extensions-dir "${HOME}/.vscode-server/extensions"

echo ""
echo "Installed.  Reload the attached VS Code window to pick it up."
echo "Verify:  ps -eo args | grep cinnabar-lsp"
