const fs = require("node:fs");
const path = require("node:path");

const COMPOSE_FILE = "compose.dev.yaml";
const WORKTREE_ENV_FILE = path.join("container", "local", "main", "worktree.env");
const SERVER_SEGMENTS = ["target", "debug", "cinnabar-lsp"];
const CONTAINER_SERVER_PATH = `./${SERVER_SEGMENTS.join("/")}`;
// Rebuilds the server before serving, so a reloaded window cannot talk to a
// binary Cargo replaced hours ago.  Preferred when present; the raw binary
// still works for a checkout that predates it.
const WRAPPER_SEGMENTS = ["container", "bin", "cinnabar-lsp-nix"];
// compose.dev.yaml sets this in the dev service's environment.  An explicit
// marker beats sniffing for /.dockerenv or a missing docker binary: those also
// match unrelated containers that cannot serve this repository.
const IN_CONTAINER_ENV_VAR = "CINNABAR_IN_DEV_CONTAINER";
// Compose mode needs both of these: the file it passes to `-f` and the
// generated environment file it passes to `--env-file`.
const COMPOSE_ROOT_MARKERS = [COMPOSE_FILE, WORKTREE_ENV_FILE];
// A relative `cinnabar.server.path` resolves against the repository root, and
// that root must be discoverable in a checkout that has generated nothing:
// `container/local/**` is ignored and absent until configure-worktree.sh runs,
// so the Compose markers cannot serve this purpose.  `flake.nix` and
// `Cargo.toml` are both tracked and both sit at the root.
const NATIVE_ROOT_MARKERS = ["flake.nix", "Cargo.toml"];

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function isInsideDevContainer(env) {
  return isNonEmptyString((env || {})[IN_CONTAINER_ENV_VAR]);
}

function findRootWithMarkers(markers, workspaceFolders, pathExists = fs.existsSync) {
  const folders = Array.isArray(workspaceFolders) ? workspaceFolders : [];

  for (const workspaceFolder of folders) {
    if (!isNonEmptyString(workspaceFolder)) {
      continue;
    }

    let candidate = path.resolve(workspaceFolder);
    while (true) {
      if (markers.every((marker) => pathExists(path.join(candidate, marker)))) {
        return candidate;
      }

      const parent = path.dirname(candidate);
      if (parent === candidate) {
        break;
      }
      candidate = parent;
    }
  }

  return undefined;
}

function findRepositoryRoot(workspaceFolders, pathExists = fs.existsSync) {
  return findRootWithMarkers(COMPOSE_ROOT_MARKERS, workspaceFolders, pathExists);
}

function directLaunchPlan(command) {
  return { command, args: [] };
}

function createLaunchPlan({ mode, serverPath, workspaceFolders, pathExists, env = process.env }) {
  const requestedMode = isNonEmptyString(mode) ? mode : "auto";
  const configuredPath = isNonEmptyString(serverPath) ? serverPath : "";

  if (requestedMode === "auto") {
    return configuredPath === ""
      ? directLaunchPlan("cinnabar-lsp")
      : directLaunchPlan(configuredPath);
  }

  if (requestedMode === "installed") {
    return directLaunchPlan("cinnabar-lsp");
  }

  if (requestedMode === "path") {
    if (configuredPath === "") {
      throw new Error("cinnabar.server.path must be set when cinnabar.server.mode is 'path'.");
    }
    if (path.isAbsolute(configuredPath)) {
      return directLaunchPlan(configuredPath);
    }
    // A relative setting names a path in the repository, not in whatever
    // directory the extension host happened to start in.  Spawning it as
    // written resolves it against that unrelated cwd and fails with a bare
    // ENOENT, which reports a misconfigured editor as a missing server.
    const repositoryRoot = findRootWithMarkers(
      NATIVE_ROOT_MARKERS,
      workspaceFolders,
      pathExists
    );
    if (repositoryRoot === undefined) {
      throw new Error(
        `A relative cinnabar.server.path ('${configuredPath}') resolves against the repository root, but no workspace folder sits at or below a Cinnabar checkout containing flake.nix and Cargo.toml. Set an absolute path instead.`
      );
    }
    return {
      command: path.join(repositoryRoot, configuredPath),
      args: [],
      options: { cwd: repositoryRoot }
    };
  }

  if (requestedMode === "docker-compose") {
    const repositoryRoot = findRepositoryRoot(workspaceFolders, pathExists);
    if (repositoryRoot === undefined) {
      throw new Error(
        "Docker Compose mode requires a workspace at or below a Cinnabar repository containing compose.dev.yaml and container/local/main/worktree.env."
      );
    }
    // When the editor is itself attached to the dev container the server is
    // already local, and the hop out through Compose is not merely redundant:
    // the service mounts no Docker socket and ships no docker CLI, so it would
    // always fail.  Run the same binary the Compose hop would have run.
    if (isInsideDevContainer(env)) {
      const exists = typeof pathExists === "function" ? pathExists : fs.existsSync;
      const wrapper = path.join(repositoryRoot, ...WRAPPER_SEGMENTS);
      if (exists(wrapper)) {
        return { command: wrapper, args: [], options: { cwd: repositoryRoot } };
      }
      const serverBinary = path.join(repositoryRoot, ...SERVER_SEGMENTS);
      // A fresh target volume has no server yet.  Spawning it anyway surfaces a
      // bare ENOENT, so name the binary and the command that produces it.
      if (!exists(serverBinary)) {
        throw new Error(
          `No language server at ${serverBinary}. Build it inside the dev container with 'nix develop --command cargo build --bin cinnabar-lsp'.`
        );
      }
      return {
        command: serverBinary,
        args: [],
        options: { cwd: repositoryRoot }
      };
    }
    return {
      command: "docker",
      args: [
        "compose",
        "--env-file",
        WORKTREE_ENV_FILE,
        "-f",
        COMPOSE_FILE,
        "exec",
        "-T",
        "dev",
        CONTAINER_SERVER_PATH
      ],
      options: { cwd: repositoryRoot }
    };
  }

  throw new Error(`Unsupported cinnabar.server.mode '${requestedMode}'.`);
}

module.exports = {
  createLaunchPlan,
  findRepositoryRoot,
  findRootWithMarkers
};
