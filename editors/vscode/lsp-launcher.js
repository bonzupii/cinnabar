const fs = require("node:fs");
const path = require("node:path");

const COMPOSE_FILE = "compose.dev.yaml";
const WORKTREE_ENV_FILE = path.join("container", "local", "main", "worktree.env");
const CONTAINER_SERVER_PATH = "./target/debug/cinnabar-lsp";

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function findRepositoryRoot(workspaceFolders, pathExists = fs.existsSync) {
  const folders = Array.isArray(workspaceFolders) ? workspaceFolders : [];

  for (const workspaceFolder of folders) {
    if (!isNonEmptyString(workspaceFolder)) {
      continue;
    }

    let candidate = path.resolve(workspaceFolder);
    while (true) {
      const composeFile = path.join(candidate, COMPOSE_FILE);
      const environmentFile = path.join(candidate, WORKTREE_ENV_FILE);
      if (pathExists(composeFile) && pathExists(environmentFile)) {
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

function directLaunchPlan(command) {
  return { command, args: [] };
}

function createLaunchPlan({ mode, serverPath, workspaceFolders, pathExists }) {
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
    return directLaunchPlan(configuredPath);
  }

  if (requestedMode === "docker-compose") {
    const repositoryRoot = findRepositoryRoot(workspaceFolders, pathExists);
    if (repositoryRoot === undefined) {
      throw new Error(
        "Docker Compose mode requires a workspace at or below a Cinnabar repository containing compose.dev.yaml and container/local/main/worktree.env."
      );
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
  findRepositoryRoot
};
