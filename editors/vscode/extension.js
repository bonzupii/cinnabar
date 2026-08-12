const vscode = require("vscode");
const languageClient = require("vscode-languageclient/node");
const { createLaunchPlan } = require("./lsp-launcher");

let client;

function activate(context) {
  const configuration = vscode.workspace.getConfiguration("cinnabar");
  const workspaceFolders = (vscode.workspace.workspaceFolders || []).map(
    (workspaceFolder) => workspaceFolder.uri.fsPath
  );
  let launchPlan;
  try {
    launchPlan = createLaunchPlan({
      mode: configuration.get("server.mode", "auto"),
      serverPath: configuration.get("server.path", ""),
      workspaceFolders
    });
  } catch (error) {
    vscode.window.showErrorMessage(`Unable to start Cinnabar Language Server: ${error.message}`);
    return;
  }
  const serverOptions = {
    ...launchPlan,
    transport: languageClient.TransportKind.stdio
  };
  const clientOptions = {
    documentSelector: [
      {
        scheme: "file",
        language: "cinnabar"
      }
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.cnb")
    }
  };
  client = new languageClient.LanguageClient(
    "cinnabar",
    "Cinnabar Language Server",
    serverOptions,
    clientOptions
  );
  context.subscriptions.push(client.start());
}

function deactivate() {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

module.exports = {
  activate,
  deactivate
};
