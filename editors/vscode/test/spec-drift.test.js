// The extension restates parts of the language: the grammar lists keywords,
// the language configuration lists comment tokens, the manifest claims a file
// extension.  None of that is generated, so it drifts silently the moment the
// compiler grows a keyword or a builtin type -- highlighting quietly goes
// wrong and nothing fails.  These tests read the compiler's own tables and
// compare, so the drift becomes a test failure instead of a mystery.
const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const extensionRoot = path.join(__dirname, "..");
const repositoryRoot = path.join(extensionRoot, "..", "..");

function readRepositoryFile(...segments) {
  return fs.readFileSync(path.join(repositoryRoot, ...segments), "utf8");
}

function readExtensionJson(...segments) {
  return JSON.parse(fs.readFileSync(path.join(extensionRoot, ...segments), "utf8"));
}

// Scopes whose patterns are plain keyword alternations.  The name-capture
// rules (`\b(fun)\s+([a-z]...)`) are deliberately excluded: they exist to
// colour the *name* after a keyword, not to enumerate the keyword set.
const KEYWORD_SCOPES = [
  "keyword.control.cinnabar",
  "keyword.declaration.cinnabar",
  "storage.modifier.cinnabar",
  "constant.language.boolean.cinnabar",
];

// Keywords the parser accepts positionally rather than reserving outright, so
// they never reach the completion table.  Each one is asserted to still be
// live in the parser below, so this list cannot outlive the syntax it covers.
const CONTEXTUAL_KEYWORDS = new Set(["for", "mut"]);

function grammarRules() {
  const grammar = readExtensionJson("syntaxes", "cinnabar.tmLanguage.json");
  const rules = [];
  const walk = (node) => {
    if (Array.isArray(node)) {
      node.forEach(walk);
      return;
    }
    if (node === null || typeof node !== "object") {
      return;
    }
    if (typeof node.name === "string" && typeof node.match === "string") {
      rules.push({ name: node.name, match: node.match });
    }
    Object.values(node).forEach(walk);
  };
  walk(grammar);
  return rules;
}

// The words inside a `\b(?:a|b|c)\b` alternation.
function alternatives(pattern) {
  const group = /\(\?:([^)]*)\)/.exec(pattern);
  if (group === null) {
    return [];
  }
  return group[1].split("|").filter((word) => /^[A-Za-z][A-Za-z0-9]*$/.test(word));
}

function grammarWordsForScopes(scopes) {
  const words = new Set();
  for (const rule of grammarRules()) {
    if (scopes.includes(rule.name)) {
      alternatives(rule.match).forEach((word) => words.add(word));
    }
  }
  return words;
}

function compilerKeywords() {
  const source = readRepositoryFile("src", "analysis.rs");
  const block = /const KEYWORDS: &\[&str\] = &\[([\s\S]*?)\];/.exec(source);
  assert.notEqual(block, null, "could not find the KEYWORDS table in src/analysis.rs");
  const words = [...block[1].matchAll(/"([a-z]+)"/g)].map((match) => match[1]);
  assert.ok(words.length > 0, "the KEYWORDS table parsed as empty");
  return new Set(words);
}

function compilerBuiltinTypes() {
  const source = readRepositoryFile("src", "resolver.rs");
  const ints = /fn builtin_int_names\([\s\S]*?\n\}/.exec(source);
  assert.notEqual(ints, null, "could not find builtin_int_names in src/resolver.rs");
  const block = /fn seed_builtins\([\s\S]*?\n\}/.exec(source);
  assert.notEqual(block, null, "could not find seed_builtins in src/resolver.rs");
  const names = new Set();
  for (const match of ints[0].matchAll(/intern\(names, "([A-Za-z0-9_]+)"\)/g)) {
    names.add(match[1]);
  }
  const boolName = /intern\(state\.0, "([A-Za-z0-9_]+)"\)/.exec(block[0]);
  assert.notEqual(boolName, null, "could not find the Bool intern in src/resolver.rs");
  names.add(boolName[1]);
  for (const match of block[0].matchAll(/seed_primitive\(state, root, "([A-Za-z0-9_]+)"/g)) {
    names.add(match[1]);
  }
  assert.ok(names.size > 0, "seed_builtins parsed as interning nothing");
  return names;
}

test("every compiler keyword is highlighted by the grammar", () => {
  const highlighted = grammarWordsForScopes(KEYWORD_SCOPES);
  const missing = [...compilerKeywords()].filter((word) => !highlighted.has(word)).sort();
  assert.deepEqual(
    missing,
    [],
    `src/analysis.rs KEYWORDS the grammar does not highlight: ${missing.join(", ")}`
  );
});

test("the grammar highlights no keyword the language does not have", () => {
  const keywords = compilerKeywords();
  const invented = [...grammarWordsForScopes(KEYWORD_SCOPES)]
    .filter((word) => !keywords.has(word) && !CONTEXTUAL_KEYWORDS.has(word))
    .sort();
  assert.deepEqual(
    invented,
    [],
    `grammar highlights words that are neither keywords nor contextual: ${invented.join(", ")}`
  );
});

test("each contextual keyword is still accepted by the parser", () => {
  const parser = readRepositoryFile("src", "parser.rs");
  for (const word of CONTEXTUAL_KEYWORDS) {
    // Require the word inside a token-matching call rather than anywhere in
    // the file: a bare substring search stays green when the keyword survives
    // only in a comment or an error message.
    const matched = new RegExp(`(?:expect|accept|is_name)\\([^)]*"${word}"`);
    assert.ok(
      matched.test(parser),
      `'${word}' is allowed as a contextual keyword but src/parser.rs no longer matches it as ` +
        "a token; drop it from CONTEXTUAL_KEYWORDS and from the grammar"
    );
  }
});

test("every builtin type the typechecker seeds is highlighted", () => {
  const typeWords = grammarWordsForScopes([
    "support.type.primitive.cinnabar",
    "constant.language.cinnabar",
  ]);
  const missing = [...compilerBuiltinTypes()].filter((name) => !typeWords.has(name)).sort();
  assert.deepEqual(
    missing,
    [],
    `seed_builtins types the grammar does not highlight: ${missing.join(", ")}`
  );
});

test("the manifest claims the file extension the repository actually uses", () => {
  const manifest = readExtensionJson("package.json");
  const languages = manifest.contributes.languages;
  const cinnabar = languages.find((entry) => entry.id === "cinnabar");
  assert.notEqual(cinnabar, undefined, "no 'cinnabar' language contribution");
  assert.deepEqual(cinnabar.extensions, [".cnb"]);

  // The fixtures are the corpus the compiler is tested against; if they ever
  // moved to another suffix the manifest above would silently stop matching.
  //
  // Ask git rather than the filesystem: compiling a fixture leaves an
  // extensionless binary beside it and `--emit-llvm` leaves a .ll, both of
  // which .gitignore documents as routine debris.  Reading the directory would
  // report that debris as a fixture with the wrong extension, so a developer
  // who had merely built the spec fixture would see this test fail.
  const tracked = execFileSync("git", ["ls-files", "tests/fixtures"], {
    cwd: repositoryRoot,
    encoding: "utf8",
  })
    .split("\n")
    .filter((line) => line.length > 0);
  assert.ok(tracked.length > 0, "git reported no tracked fixtures");
  const wrongExtension = tracked.filter(
    (file) => !file.endsWith(".cnb") && !file.endsWith(".idl")
  );
  assert.deepEqual(
    wrongExtension,
    [],
    `tracked fixtures that are neither .cnb nor .idl: ${wrongExtension.slice(0, 5).join(", ")}`
  );
});

test("the grammar's comment rules match the language configuration", () => {
  const configuration = readExtensionJson("language-configuration.json");
  const patterns = grammarRules().map((rule) => rule.name);
  const beginRules = JSON.stringify(
    readExtensionJson("syntaxes", "cinnabar.tmLanguage.json")
  );

  assert.equal(configuration.comments.lineComment, "#");
  assert.deepEqual(configuration.comments.blockComment, ["#|", "|#"]);
  // The grammar must actually implement both forms the configuration promises.
  assert.ok(
    beginRules.includes("#\\\\|") || beginRules.includes("#\\|"),
    "language-configuration declares a #| block comment the grammar does not open"
  );
  assert.ok(
    patterns.includes("comment.line.number-sign.cinnabar"),
    "language-configuration declares a # line comment the grammar does not scope"
  );
});

test("indentation rules mention only real keywords", () => {
  const configuration = readExtensionJson("language-configuration.json");
  const keywords = compilerKeywords();
  const rules = [
    configuration.indentationRules.increaseIndentPattern,
    configuration.indentationRules.decreaseIndentPattern,
  ];
  const unknown = new Set();
  for (const rule of rules) {
    for (const word of rule.matchAll(/[a-z]{2,}/g)) {
      // `s` and other regex fragments are filtered by the length bound; what
      // remains is either a keyword or a stray token worth failing on.
      if (!keywords.has(word[0]) && !CONTEXTUAL_KEYWORDS.has(word[0])) {
        unknown.add(word[0]);
      }
    }
  }
  assert.deepEqual(
    [...unknown].sort(),
    [],
    `indentation rules reference non-keywords: ${[...unknown].join(", ")}`
  );
});

// Every reserved word a snippet body relies on structurally (not the English
// placeholder text inside `${n:...}` tabstops, which a scan can't tell apart
// from real syntax). Mirrors CONTEXTUAL_KEYWORDS above: a hand-kept set
// cross-checked against the compiler's own table, rather than an attempt to
// parse "is this token code or prose" out of the snippet bodies themselves.
const SNIPPET_KEYWORDS = [
  "fun", "end", "if", "elif", "else", "while", "match", "pub", "nat", "const",
  "val", "var", "use", "type", "mod", "trait", "impl",
];

test("snippet bodies reference only real keywords", () => {
  const keywords = compilerKeywords();
  const missing = SNIPPET_KEYWORDS.filter((word) => !keywords.has(word)).sort();
  assert.deepEqual(
    missing,
    [],
    `snippets/cinnabar.json assumes keywords src/analysis.rs no longer has: ${missing.join(", ")}`
  );
});

test("every snippet has a unique prefix and the manifest points at a real file", () => {
  const manifest = readExtensionJson("package.json");
  const declared = manifest.contributes.snippets.find((entry) => entry.language === "cinnabar");
  assert.notEqual(declared, undefined, "no 'cinnabar' snippets contribution");
  assert.ok(
    fs.existsSync(path.join(extensionRoot, declared.path)),
    `package.json points snippets at ${declared.path}, which does not exist`
  );

  const snippets = readExtensionJson(declared.path.replace(/^\.\//, ""));
  const prefixes = Object.values(snippets).map((entry) => entry.prefix);
  const duplicates = prefixes.filter((prefix, index) => prefixes.indexOf(prefix) !== index);
  assert.deepEqual([...new Set(duplicates)], [], `duplicate snippet prefixes: ${duplicates.join(", ")}`);
});

test("the launcher targets a binary the workspace actually builds", () => {
  const launcher = fs.readFileSync(path.join(extensionRoot, "lsp-launcher.js"), "utf8");
  const segments = /const SERVER_SEGMENTS = \[([^\]]*)\]/.exec(launcher);
  assert.notEqual(segments, null, "could not find SERVER_SEGMENTS in lsp-launcher.js");
  const parts = [...segments[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  const binaryName = parts[parts.length - 1];

  const cargo = readRepositoryFile("Cargo.toml");
  const declared = [...cargo.matchAll(/\[\[bin\]\][\s\S]*?name\s*=\s*"([^"]+)"/g)].map((m) => m[1]);
  assert.ok(
    declared.includes(binaryName),
    `lsp-launcher.js launches '${binaryName}', which Cargo.toml does not declare: ${declared.join(", ")}`
  );

  // The wrapper is what actually runs in the container, so it must build and
  // exec the same binary the launcher would otherwise have spawned directly.
  const wrapper = readRepositoryFile("container", "bin", "cinnabar-lsp-nix");
  assert.ok(
    wrapper.includes(`--bin ${binaryName}`) && wrapper.includes(`/${binaryName}`),
    `container/bin/cinnabar-lsp-nix does not build and exec '${binaryName}'`
  );
});
