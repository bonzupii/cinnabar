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
  // `client.start()` returns a Promise<void> under vscode-languageclient ^9,
  // not a Disposable -- pushing it into context.subscriptions makes VS Code
  // call .dispose() on a Promise when the extension deactivates, which
  // throws. deactivate() below already stops the client, so start() needs
  // nothing pushed; it only needs its rejection surfaced instead of left
  // unhandled.
  client.start().catch((error) => {
    vscode.window.showErrorMessage(`Cinnabar Language Server failed to start: ${error.message}`);
  });
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
