const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { createLaunchPlan, findRepositoryRoot } = require("../lsp-launcher");

const repositoryRoot = path.join(process.cwd(), "fixture-workspace", "cinnabar");
const composeFile = path.join(repositoryRoot, "compose.dev.yaml");
const environmentFile = path.join(
  repositoryRoot,
  "container",
  "local",
  "main",
  "worktree.env"
);

function repositoryPathExists(candidate) {
  return candidate === composeFile || candidate === environmentFile;
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
      serverPath: "/opt/cinnabar/bin/cinnabar-lsp",
      workspaceFolders: []
    }),
    { command: "/opt/cinnabar/bin/cinnabar-lsp", args: [] }
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
      pathExists: repositoryPathExists
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
        pathExists: () => false
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
  assert.deepEqual(workspaceSettings, { "cinnabar.server.mode": "docker-compose" });
  assert.doesNotMatch(JSON.stringify(workspaceSettings), /(?:[Tt]emp|cinnabar-lsp-docker)/);
});
