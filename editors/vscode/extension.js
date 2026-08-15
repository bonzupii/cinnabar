const vscode = require("vscode");
const languageClient = require("vscode-languageclient/node");
const { createLaunchPlan } = require("./lsp-launcher");

let client;
let statusBarItem;

// vscode-languageclient's State enum (Stopped = 1, Starting = 3, Running = 5)
// isn't re-exported through a require() we can destructure cleanly here, so
// the status bar renders straight off the numeric State it hands
// onDidChangeState rather than importing the enum for three labels.
const STATE_PRESENTATION = {
  [languageClient.State.Stopped]: { text: "$(circle-slash) Cinnabar", tooltip: "Cinnabar Language Server is stopped" },
  [languageClient.State.Starting]: { text: "$(sync~spin) Cinnabar", tooltip: "Cinnabar Language Server is starting…" },
  [languageClient.State.Running]: { text: "$(check) Cinnabar", tooltip: "Cinnabar Language Server is running" }
};

function renderState(state) {
  if (!statusBarItem) {
    return;
  }
  const presentation = STATE_PRESENTATION[state] || STATE_PRESENTATION[languageClient.State.Stopped];
  statusBarItem.text = presentation.text;
  statusBarItem.tooltip = presentation.tooltip;
}

function startClient() {
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
  client.onDidChangeState((event) => renderState(event.newState));
  renderState(languageClient.State.Starting);
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

async function restartServer() {
  if (!client) {
    startClient();
    return;
  }
  try {
    await client.stop();
  } catch (error) {
    // Falls through to starting a fresh client regardless of how the old
    // one shut down -- a client wedged mid-stop is exactly what "restart"
    // needs to recover from.
  }
  startClient();
}

function showOutputChannel() {
  if (!client) {
    vscode.window.showInformationMessage("Cinnabar Language Server has not started yet.");
    return;
  }
  client.outputChannel.show();
}

function activate(context) {
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 0);
  statusBarItem.command = "cinnabar.showOutputChannel";
  renderState(languageClient.State.Stopped);
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  context.subscriptions.push(
    vscode.commands.registerCommand("cinnabar.showExplanation", (explanation) => {
      vscode.window.showInformationMessage(explanation);
    }),
    vscode.commands.registerCommand("cinnabar.restartServer", restartServer),
    vscode.commands.registerCommand("cinnabar.showOutputChannel", showOutputChannel)
  );

  startClient();
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
