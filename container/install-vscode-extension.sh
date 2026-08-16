#!/usr/bin/env bash
# Packages editors/vscode and installs it into the VS Code Server this machine
# runs, so the editor starts the Cinnabar language server with no manual steps.
# The extension is not on the Marketplace, so no configuration can pull it by
# identifier; it has to be built and installed from the checkout.
#
# Runs wherever that server and `node` are, which is a WSL2 distro under the
# default workflow and the dev container under the fallback.  Nothing here is
# container-specific: `nix develop` supplies node, and the server directory is
# discovered below.  In the container the extension lands in the
# cinnabar-vscode-server volume and survives service recreation; re-run this
# after changing the extension either way.
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXTENSION_ROOT="${REPOSITORY_ROOT}/editors/vscode"
VSIX="${TMPDIR:-/tmp}/cinnabar-language-support.vsix"

# No container check here.  What this script needs is a VS Code Server and a
# node, both asserted directly below; a container is one place to find them and
# a WSL2 distro is another.  Testing for the container instead refused to run in
# the environment that most needs it -- a WSL-remote window still has to install
# this vsix, because the extension is not on the Marketplace.
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
  (cd "${EXTENSION_ROOT}" && npm ci --no-audit --no-fund)
fi

echo "Packaging extension..."
# vsce stays an npx fetch rather than a devDependency: package.json lists
# node_modules in "files", so anything installed here ships inside the vsix.
(cd "${EXTENSION_ROOT}" && npx --yes @vscode/vsce package --allow-missing-repository --out "${VSIX}")

# The server directory is named for the VS Code commit, which changes whenever
# the editor updates, so discover it rather than pinning it.
SERVER_CLI="$(find "${HOME}/.vscode-server/bin" -mindepth 3 -maxdepth 3 -type f -name code-server -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -n1 | cut -d' ' -f2- || true)"
if [ -z "${SERVER_CLI}" ]; then
  echo "No VS Code Server under ${HOME}/.vscode-server." >&2
  echo "Open this checkout in the editor once so the server installs itself," >&2
  echo "then re-run this script:" >&2
  echo "" >&2
  echo "  WSL2:      code --remote wsl+<distro> ${REPOSITORY_ROOT}" >&2
  echo "  container: Dev Containers: Attach to Running Container..." >&2
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
