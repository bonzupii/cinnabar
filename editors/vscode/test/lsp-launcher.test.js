const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const {
  createLaunchPlan,
  findRepositoryRoot,
  findRootWithMarkers
} = require("../lsp-launcher");

const repositoryRoot = path.join(process.cwd(), "fixture-workspace", "cinnabar");
const composeFile = path.join(repositoryRoot, "compose.dev.yaml");
const environmentFile = path.join(
  repositoryRoot,
  "container",
  "local",
  "main",
  "worktree.env"
);

const serverBinary = path.join(repositoryRoot, "target", "debug", "cinnabar-lsp");

function repositoryPathExists(candidate) {
  return candidate === composeFile || candidate === environmentFile;
}

function builtRepositoryPathExists(candidate) {
  return repositoryPathExists(candidate) || candidate === serverBinary;
}

// A checkout that has generated nothing: the tracked root files are present and
// container/local/** is not.
function nativeRepositoryPathExists(candidate) {
  return (
    candidate === path.join(repositoryRoot, "flake.nix") ||
    candidate === path.join(repositoryRoot, "Cargo.toml")
  );
}

test("auto mode uses the installed cinnabar-lsp command without a configured path", () => {
  assert.deepEqual(
    createLaunchPlan({ mode: "auto", serverPath: "", workspaceFolders: [] }),
    { command: "cinnabar-lsp", args: [] }
  );
});

test("auto mode preserves an existing explicit cinnabar-lsp path", () => {
  assert.deepEqual(
    createLaunchPlan({
      mode: "auto",
      serverPath: "C:\\tools\\cinnabar-lsp.exe",
      workspaceFolders: []
    }),
    { command: "C:\\tools\\cinnabar-lsp.exe", args: [] }
  );
});

test("installed mode chooses PATH even when a direct path is configured", () => {
  assert.deepEqual(
    createLaunchPlan({
      mode: "installed",
      serverPath: "C:\\tools\\cinnabar-lsp.exe",
      workspaceFolders: []
    }),
    { command: "cinnabar-lsp", args: [] }
  );
});

test("path mode requires and uses the configured executable", () => {
  assert.throws(
    () => createLaunchPlan({ mode: "path", serverPath: "", workspaceFolders: [] }),
    /cinnabar\.server\.path must be set/
  );
  assert.deepEqual(
    createLaunchPlan({
      mode: "path",
      serverPath: path.join(path.sep, "opt", "cinnabar", "bin", "cinnabar-lsp"),
      workspaceFolders: []
    }),
    { command: path.join(path.sep, "opt", "cinnabar", "bin", "cinnabar-lsp"), args: [] }
  );
});

test("path mode resolves a relative executable against the repository, not the editor cwd", () => {
  const wrapper = path.join("container", "bin", "cinnabar-lsp-nix");
  assert.deepEqual(
    createLaunchPlan({
      mode: "path",
      serverPath: wrapper,
      workspaceFolders: [path.join(repositoryRoot, "editors", "vscode")],
      pathExists: nativeRepositoryPathExists
    }),
    {
      command: path.join(repositoryRoot, wrapper),
      args: [],
      options: { cwd: repositoryRoot }
    }
  );
});

test("path mode finds the repository without any generated container file", () => {
  // A fresh clone has no container/local/**, so a root discovered only by the
  // Compose markers would be unreachable here.
  assert.equal(nativeRepositoryPathExists(environmentFile), false);
  assert.equal(
    findRootWithMarkers(
      ["flake.nix", "Cargo.toml"],
      [path.join(repositoryRoot, "editors", "vscode")],
      nativeRepositoryPathExists
    ),
    repositoryRoot
  );
});

test("a relative path outside any checkout names the setting and the fix", () => {
  assert.throws(
    () =>
      createLaunchPlan({
        mode: "path",
        serverPath: path.join("container", "bin", "cinnabar-lsp-nix"),
        workspaceFolders: [path.join(path.sep, "workspace", "other")],
        pathExists: () => false
      }),
    /relative cinnabar\.server\.path .*flake\.nix and Cargo\.toml.*absolute path/s
  );
});

test("Docker Compose mode finds the repository above a nested workspace and uses the direct stdio contract", () => {
  assert.equal(
    findRepositoryRoot([path.join(repositoryRoot, "editors", "vscode")], repositoryPathExists),
    repositoryRoot
  );
  assert.deepEqual(
    createLaunchPlan({
      mode: "docker-compose",
      serverPath: "",
      workspaceFolders: [path.join(repositoryRoot, "editors", "vscode")],
      pathExists: repositoryPathExists,
      env: {}
    }),
    {
      command: "docker",
      args: [
        "compose",
        "--env-file",
        path.join("container", "local", "main", "worktree.env"),
        "-f",
        "compose.dev.yaml",
        "exec",
        "-T",
        "dev",
        "./target/debug/cinnabar-lsp"
      ],
      options: { cwd: repositoryRoot }
    }
  );
});

test("Docker Compose mode rejects workspaces without the repository-owned launch files", () => {
  assert.throws(
    () =>
      createLaunchPlan({
        mode: "docker-compose",
        serverPath: "",
        workspaceFolders: [path.join(path.sep, "workspace", "other")],
        pathExists: () => false,
        env: {}
      }),
    /Docker Compose mode requires a workspace/
  );
});

test("Docker Compose mode runs the server directly when the editor is attached to the dev container", () => {
  assert.deepEqual(
    createLaunchPlan({
      mode: "docker-compose",
      serverPath: "",
      workspaceFolders: [path.join(repositoryRoot, "editors", "vscode")],
      pathExists: builtRepositoryPathExists,
      env: { CINNABAR_IN_DEV_CONTAINER: "1" }
    }),
    {
      command: serverBinary,
      args: [],
      options: { cwd: repositoryRoot }
    }
  );
});

test("the rebuilding wrapper is preferred over the raw binary when present", () => {
  const wrapper = path.join(repositoryRoot, "container", "bin", "cinnabar-lsp-nix");
  assert.deepEqual(
    createLaunchPlan({
      mode: "docker-compose",
      serverPath: "",
      workspaceFolders: [path.join(repositoryRoot, "editors", "vscode")],
      pathExists: (candidate) => builtRepositoryPathExists(candidate) || candidate === wrapper,
      env: { CINNABAR_IN_DEV_CONTAINER: "1" },
    }),
    { command: wrapper, args: [], options: { cwd: repositoryRoot } }
  );
});

test("an unbuilt server in the container names the binary and the build command", () => {
  assert.throws(
    () =>
      createLaunchPlan({
        mode: "docker-compose",
        serverPath: "",
        workspaceFolders: [path.join(repositoryRoot, "editors", "vscode")],
        pathExists: repositoryPathExists,
        env: { CINNABAR_IN_DEV_CONTAINER: "1" }
      }),
    /No language server at .*cinnabar-lsp.*cargo build --bin cinnabar-lsp/s
  );
});

test("the in-container shortcut still requires a resolvable repository root", () => {
  assert.throws(
    () =>
      createLaunchPlan({
        mode: "docker-compose",
        serverPath: "",
        workspaceFolders: [path.join(path.sep, "workspace", "other")],
        pathExists: () => false,
        env: { CINNABAR_IN_DEV_CONTAINER: "1" }
      }),
    /Docker Compose mode requires a workspace/
  );
});

test("the checked-in extension package owns the launcher and workspace settings stay portable", () => {
  const packagePath = path.join(__dirname, "..", "package.json");
  const settingsPath = path.join(__dirname, "..", "..", "..", ".vscode", "settings.json");
  const extensionPackage = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  const workspaceSettings = JSON.parse(fs.readFileSync(settingsPath, "utf8"));

  assert.equal(extensionPackage.scripts.test, "node --test");
  assert.equal(extensionPackage.files.includes("lsp-launcher.js"), true);
  // The checked-in default drives the repository's own toolchain without a
  // container: the wrapper runs the server through `nix develop`, which is what
  // CI uses too.  Compose mode remains selectable but cannot be the default —
  // it needs container/local/**, which a fresh clone does not have.
  assert.deepEqual(workspaceSettings, {
    "cinnabar.server.mode": "path",
    "cinnabar.server.path": path.posix.join("container", "bin", "cinnabar-lsp-nix")
  });
  assert.doesNotMatch(JSON.stringify(workspaceSettings), /(?:[Tt]emp|cinnabar-lsp-docker)/);
});
