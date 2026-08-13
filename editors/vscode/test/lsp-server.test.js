// The launcher tests prove the extension computes the right command; they do
// not prove that command produces a working language server.  This drives a
// real LSP session over the binary the launcher actually names -- initialize,
// didOpen, publishDiagnostics -- and checks the verdicts against the same
// fixtures the compiler's own harness classifies, so a compiler regression or
// a protocol change surfaces here rather than as a silent editor.
const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { pathToFileURL } = require("node:url");
const { createLaunchPlan } = require("../lsp-launcher");

const extensionRoot = path.join(__dirname, "..");
const repositoryRoot = path.join(extensionRoot, "..", "..");
const WAIT_MS = 30000;

// Resolve the server exactly the way an editor attached to the dev container
// would, so the test covers the launcher's output rather than a path of its
// own invention.
function resolveServer() {
  try {
    return createLaunchPlan({
      mode: "docker-compose",
      serverPath: "",
      workspaceFolders: [repositoryRoot],
      env: { CINNABAR_IN_DEV_CONTAINER: "1" },
    });
  } catch (error) {
    return { unavailable: error.message };
  }
}

function frameReader(stream) {
  let buffer = Buffer.alloc(0);
  const received = [];
  const waiters = [];

  const settle = () => {
    let index = 0;
    while (index < waiters.length) {
      const waiter = waiters[index];
      const hit = received.find(waiter.predicate);
      if (hit !== undefined) {
        waiter.resolve(hit);
        waiters.splice(index, 1);
        continue;
      }
      index += 1;
    }
  };

  stream.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) {
        break;
      }
      const header = buffer.subarray(0, headerEnd).toString("ascii");
      const length = /content-length:\s*(\d+)/i.exec(header);
      if (length === null) {
        buffer = buffer.subarray(headerEnd + 4);
        continue;
      }
      const start = headerEnd + 4;
      const end = start + Number(length[1]);
      if (buffer.length < end) {
        break;
      }
      const body = buffer.subarray(start, end).toString("utf8");
      buffer = buffer.subarray(end);
      try {
        received.push(JSON.parse(body));
      } catch {
        continue;
      }
    }
    settle();
  });

  return {
    wait(predicate, description) {
      const hit = received.find(predicate);
      if (hit !== undefined) {
        return Promise.resolve(hit);
      }
      return new Promise((resolve, reject) => {
        const waiter = { predicate, resolve };
        waiters.push(waiter);
        setTimeout(() => {
          const at = waiters.indexOf(waiter);
          if (at !== -1) {
            waiters.splice(at, 1);
            reject(new Error(`timed out waiting for ${description}`));
          }
        }, WAIT_MS).unref();
      });
    },
  };
}

function send(child, message) {
  const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...message }), "utf8");
  child.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
  child.stdin.write(body);
}

function openFixture(child, relativePath) {
  const absolute = path.join(repositoryRoot, relativePath);
  const uri = pathToFileURL(absolute).href;
  send(child, {
    method: "textDocument/didOpen",
    params: {
      textDocument: {
        uri,
        languageId: "cinnabar",
        version: 1,
        text: fs.readFileSync(absolute, "utf8"),
      },
    },
  });
  return uri;
}

function diagnosticsFor(reader, uri, label) {
  return reader.wait(
    (message) =>
      message.method === "textDocument/publishDiagnostics" && message.params?.uri === uri,
    `diagnostics for ${label}`
  );
}

const server = resolveServer();

test("the language server the launcher names answers a real LSP session", async (t) => {
  if (server.unavailable !== undefined) {
    t.skip(
      `language server not built (${server.unavailable}); ` +
        "run: nix develop --command cargo build --bin cinnabar-lsp"
    );
    return;
  }

  const child = spawn(server.command, server.args, {
    cwd: server.options?.cwd ?? repositoryRoot,
    stdio: ["pipe", "pipe", "pipe"],
  });
  t.after(() => child.kill());

  // The path can exist and still not be runnable here: the repository is
  // developed from Windows against a Linux dev container, so target/debug can
  // hold an ELF binary the host cannot exec.  That is a "run this in the
  // container" signal, not a failing assertion.
  const spawnError = await new Promise((resolve) => {
    child.once("spawn", () => resolve(null));
    child.once("error", (error) => resolve(error));
  });
  if (spawnError !== null) {
    if (["ENOENT", "EACCES", "ENOEXEC"].includes(spawnError.code)) {
      t.skip(
        `${server.command} is not runnable on ${process.platform} (${spawnError.code}); ` +
          "run these tests inside the dev container"
      );
      return;
    }
    assert.fail(`could not spawn the server: ${spawnError.message}`);
  }

  const reader = frameReader(child.stdout);

  send(child, {
    id: 1,
    method: "initialize",
    params: {
      processId: null,
      rootUri: pathToFileURL(repositoryRoot).href,
      capabilities: {},
    },
  });
  const initialized = await reader.wait((message) => message.id === 1, "the initialize response");
  const capabilities = initialized.result?.capabilities;
  assert.notEqual(capabilities, undefined, "initialize returned no capabilities");

  // stdio sync is what makes didOpen meaningful; without it the editor shows
  // nothing no matter how healthy the analyzer is.
  assert.notEqual(
    capabilities.textDocumentSync,
    undefined,
    "the server advertises no textDocumentSync, so didOpen would be ignored"
  );

  // A request the server handles but never advertises is dead code from the
  // editor's side: the client reads these capabilities to decide what to send,
  // so it would never issue the request.  Compared against the live response
  // rather than the source text, so a renamed capability key is caught.
  const providers = {
    "textDocument/hover": "hoverProvider",
    "textDocument/completion": "completionProvider",
    "textDocument/definition": "definitionProvider",
    "textDocument/references": "referencesProvider",
    "textDocument/signatureHelp": "signatureHelpProvider",
    "textDocument/codeLens": "codeLensProvider",
    "textDocument/formatting": "documentFormattingProvider",
  };
  const serverSource = fs.readFileSync(
    path.join(repositoryRoot, "src", "bin", "cinnabar_lsp.rs"),
    "utf8"
  );
  const unadvertised = Object.entries(providers)
    .filter(([method, capability]) => {
      return serverSource.includes(`"${method}"`) && capabilities[capability] === undefined;
    })
    .map(([method, capability]) => `${method} -> ${capability}`);
  assert.deepEqual(
    unadvertised,
    [],
    `handled but not advertised, so the client will never ask: ${unadvertised.join(", ")}`
  );
  // The inverse is just as broken: an advertised provider with no handler
  // makes the client send a request that comes back MethodNotFound.
  const unhandled = Object.entries(providers)
    .filter(([method, capability]) => {
      return capabilities[capability] !== undefined && !serverSource.includes(`"${method}"`);
    })
    .map(([method, capability]) => `${capability} -> ${method}`);
  assert.deepEqual(
    unhandled,
    [],
    `advertised but not handled, so the request will fail: ${unhandled.join(", ")}`
  );

  send(child, { method: "initialized", params: {} });

  // explain_leak.cnb documents itself as EXPECT_REJECTED; the editor must show
  // that rather than a clean file.
  const rejected = path.join("tests", "fixtures", "explain_leak.cnb");
  const header = fs.readFileSync(path.join(repositoryRoot, rejected), "utf8").slice(0, 400);
  assert.ok(
    header.includes("EXPECT_REJECTED"),
    `${rejected} no longer declares EXPECT_REJECTED; pick another rejected fixture`
  );
  const rejectedUri = openFixture(child, rejected);
  const rejectedDiagnostics = await diagnosticsFor(reader, rejectedUri, rejected);
  assert.ok(
    rejectedDiagnostics.params.diagnostics.length > 0,
    `${rejected} is expected to be rejected but the server reported no diagnostics`
  );

  // linear_branch_consume.cnb is listed in the expected-success corpus, so
  // the server must report it clean.  Asserting the listing keeps the two in
  // step.  The corpus lives in its own file because the repro harness and the
  // sanitizer gate both run it; reading it here rather than the harness is
  // what keeps this assertion pointed at the table itself.
  const accepted = path.join("tests", "fixtures", "repro", "linear_branch_consume.cnb");
  const corpusPath = path.join(repositoryRoot, "tests", "support", "repro_corpus.rs");
  const corpus = fs.readFileSync(corpusPath, "utf8");
  assert.ok(
    /EXPECT_OK[\s\S]*?"linear_branch_consume"[\s\S]*?\];/.test(corpus),
    "linear_branch_consume is no longer in the expected-success corpus"
  );
  const acceptedUri = openFixture(child, accepted);
  const acceptedDiagnostics = await diagnosticsFor(reader, acceptedUri, accepted);
  assert.deepEqual(
    acceptedDiagnostics.params.diagnostics,
    [],
    `${accepted} is expected to compile but the server reported diagnostics`
  );

  // Formatting round-trip.  The input is a real fixture with its indentation
  // wrecked rather than a snippet written here, so the test cannot drift from
  // the language: whatever the corpus contains is what gets formatted.
  const acceptedText = fs.readFileSync(path.join(repositoryRoot, accepted), "utf8");
  const mangled = acceptedText
    .split("\n")
    .map((line, index) => (line.trim() === "" ? line : `${index % 2 ? "   " : " "}${line.trim()}`))
    .join("\n");
  const mangledUri = pathToFileURL(path.join(repositoryRoot, "tests", "fixtures", "_fmt.cnb")).href;
  send(child, {
    method: "textDocument/didOpen",
    params: {
      textDocument: { uri: mangledUri, languageId: "cinnabar", version: 1, text: mangled },
    },
  });
  await diagnosticsFor(reader, mangledUri, "the mangled buffer");
  send(child, {
    id: 3,
    method: "textDocument/formatting",
    params: { textDocument: { uri: mangledUri }, options: { tabSize: 2, insertSpaces: true } },
  });
  const formatting = await reader.wait((message) => message.id === 3, "the formatting response");
  assert.equal(formatting.result.length, 1, "re-indenting a whole file should be one edit");
  assert.equal(
    formatting.result[0].newText,
    acceptedText,
    "formatting a wrecked copy of a fixture did not restore the fixture"
  );

  // Formatting an already-formatted buffer must be a no-op; returning a
  // whole-document replacement would dirty the file on every format-on-save.
  send(child, {
    id: 4,
    method: "textDocument/formatting",
    params: { textDocument: { uri: acceptedUri }, options: { tabSize: 2, insertSpaces: true } },
  });
  const idempotent = await reader.wait((message) => message.id === 4, "the idempotent format");
  assert.deepEqual(
    idempotent.result,
    [],
    "formatting an already-formatted document produced edits"
  );

  // Dot-completion on a buffer that does not parse.  A trailing dot is a
  // syntax error, which is exactly when an editor asks what follows it, so the
  // server must still answer with the module's members and never with keywords
  // -- none of which may follow a dot.
  const partial =
    "pub mod Widget\n" +
    "  pub nat type Handle\n" +
    "  pub nat fun open() impure Handle\n" +
    "  pub nat fun close(handle: Handle) impure Unit\n" +
    "end\n" +
    "\n" +
    "pub fun main() impure I64\n" +
    "  val w = Widget.\n" +
    "  return 0\n" +
    "end\n";
  const partialUri = pathToFileURL(path.join(repositoryRoot, "tests", "fixtures", "_dot.cnb")).href;
  send(child, {
    method: "textDocument/didOpen",
    params: {
      textDocument: { uri: partialUri, languageId: "cinnabar", version: 1, text: partial },
    },
  });
  await diagnosticsFor(reader, partialUri, "the partial buffer");
  const dotLine = partial.split("\n").findIndex((line) => line.includes("Widget."));
  send(child, {
    id: 5,
    method: "textDocument/completion",
    params: {
      textDocument: { uri: partialUri },
      position: { line: dotLine, character: partial.split("\n")[dotLine].length },
      context: { triggerKind: 2, triggerCharacter: "." },
    },
  });
  const completion = await reader.wait((message) => message.id === 5, "the completion response");
  const labels = (completion.result?.items ?? completion.result ?? []).map((item) => item.label);
  assert.deepEqual(
    ["Handle", "close", "open"].filter((name) => !labels.includes(name)),
    [],
    `dot-completion omitted members of Widget; got: ${labels.join(", ")}`
  );
  assert.deepEqual(
    ["fun", "val", "if", "while", "return"].filter((word) => labels.includes(word)),
    [],
    `dot-completion offered keywords, which cannot follow a dot: ${labels.join(", ")}`
  );

  send(child, { id: 9, method: "shutdown", params: null });
  await reader.wait((message) => message.id === 9, "the shutdown response");
  send(child, { method: "exit", params: null });
});

test("the extension's document selector matches the language it contributes", () => {
  // The client only attaches to documents whose languageId matches, so this
  // pair drifting apart silently disables the server for every .cnb file.
  const extension = fs.readFileSync(path.join(extensionRoot, "extension.js"), "utf8");
  const selector = /language:\s*"([a-z]+)"/.exec(extension);
  assert.notEqual(selector, null, "could not find the document selector in extension.js");

  const manifest = JSON.parse(fs.readFileSync(path.join(extensionRoot, "package.json"), "utf8"));
  const ids = manifest.contributes.languages.map((entry) => entry.id);
  assert.ok(
    ids.includes(selector[1]),
    `extension.js selects language '${selector[1]}', which package.json does not contribute: ${ids.join(", ")}`
  );
});
