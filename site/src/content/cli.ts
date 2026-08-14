/*
 * The CLI surface, transcribed from README.md.
 *
 * There are two invocation forms: given a source file, `cinnabar` runs the
 * whole pipeline and writes a static binary; given a subcommand, it acts on
 * the project whose build.cnb manifest is found by walking upward from the
 * supplied path.
 */

export type Row = { name: string; description: string };
export type Section = {
  id: string;
  title: string;
  note?: string;
  intro?: string;
  rows: readonly Row[];
};

export const USAGE = `cinnabar <FILE> [-o|--output PATH] [--dump-ast] [--dump-typed-ast] [--print-layout]
                [--emit-llvm] [--emit-obj] [--explain-borrow[=human|json]] [--run]
                [-O|--opt-level {0,1,2,3,s,z}]
cinnabar <COMMAND> [ARGS]`;

export const CLI_SECTIONS: readonly Section[] = [
  {
    id: "single-file",
    title: "Compiling a single file",
    note: "cinnabar <FILE> [FLAGS]",
    intro:
      "On success the compiler prints `Successfully compiled <input> to '<output>'.` and exits 0. Any lex, parse, resolve, typecheck, borrow-check or codegen failure is rendered as source-located diagnostics and exits non-zero. There is no partial output: a build either produces its artifact or produces diagnostics.",
    rows: [
      {
        name: "<FILE>",
        description:
          "Input Cinnabar source file (positional, required), conventionally .cnb",
      },
      {
        name: "-o, --output <PATH>",
        description:
          "Output binary path (defaults to the input path with .cnb stripped)",
      },
      {
        name: "--dump-ast",
        description:
          "Parse only, pretty-print the AST, and exit — no resolve, typecheck, borrow-check or codegen",
      },
      {
        name: "--dump-typed-ast",
        description:
          "Run the full front end, then print the node arena with every attached fact — resolved symbols, canonical type keys, linearity flags, variant tags, field facts — and exit",
      },
      {
        name: "--print-layout",
        description:
          "Run the full front end, then print ABI size, alignment, field offsets and enum variant tags for every concrete struct, enum and native handle, and exit",
      },
      {
        name: "--emit-llvm",
        description:
          "Write the emitter's LLVM IR (before optimization) to the input path with .ll and stop",
      },
      {
        name: "--emit-obj",
        description:
          "Optimize and assemble to a relocatable object at the input path with .o, skipping the static link",
      },
      {
        name: "--explain-borrow[=human|json]",
        description:
          "Attach secondary labels to borrow and linearity errors: which paths consume a value, where it was bound and its linear type, where it was previously moved. =json emits them as structured diagnostics instead",
      },
      {
        name: "--run",
        description:
          "Execute the produced binary after a successful build. cinnabar then exits 0 if the program exited 0, and non-zero otherwise",
      },
      {
        name: "-O, --opt-level <LEVEL>",
        description: "LLVM optimization level: 0, 1, 2, 3, s, z (default 2)",
      },
    ],
  },
  {
    id: "project",
    title: "Working on a project",
    note: "cinnabar <COMMAND> [PATH]",
    intro:
      "PATH defaults to `.` and may be a project directory, a build.cnb, or a source path inside the project — the manifest is found by walking upward from it either way. `--target` currently accepts only `host`; run `cinnabar targets` for the list and the state of each.",
    rows: [
      {
        name: "cinnabar init [PATH]",
        description:
          "Scaffold build.cnb, main.cnb and tests/smoke.cnb. Refuses to overwrite: if any of the three exists, it writes none of them",
      },
      {
        name: "cinnabar build [PATH] [--target host]",
        description: "Compile the manifest's ENTRY to <project>/target/<NAME>",
      },
      {
        name: "cinnabar run [PATH] [--target host]",
        description:
          "Build, then execute the artifact. Exits 0 if the program exited 0, non-zero otherwise",
      },
      {
        name: "cinnabar check [PATH]",
        description:
          "Load, resolve, typecheck and borrow-check; stop before code generation. Needs no LLVM and links nothing",
      },
      {
        name: "cinnabar test [PATH] [--update-snapshots]",
        description:
          "Compile and run every .cnb file under the manifest's TESTS directory, recursively",
      },
      {
        name: "cinnabar fmt [--check] <FILE>",
        description:
          "Rewrite one file into canonical form, or with --check exit non-zero if it is not already canonical",
      },
      {
        name: "cinnabar doc [PATH] [-o DIR]",
        description:
          "Render every public declaration into <project>/target/doc/index.html",
      },
      {
        name: "cinnabar burn [PATH] [--address ADDR]",
        description:
          "Serve those docs plus the manifesto over HTTP, pinned to this compiler's version (default 127.0.0.1:7878)",
      },
    ],
  },
  {
    id: "inspect",
    title: "Inspecting and experimenting",
    note: "Read-only and local-only surfaces",
    rows: [
      {
        name: "cinnabar targets",
        description:
          "List code-generation targets and whether this binary can build for each",
      },
      {
        name: "cinnabar inspect [PATH] [-o FILE]",
        description:
          "Build, then report computed layouts alongside the linked binary's sections, symbols and disassembly",
      },
      {
        name: "cinnabar soundness [PATH] [-o FILE]",
        description:
          "Emit what the front end established, as JSON. Evidence, not a proof — the report says formal_proof: false and scopes itself",
      },
      {
        name: "cinnabar playground [--address ADDR]",
        description:
          "Serve a local page that compiles and runs submitted source. Loopback-only, size-capped and time-limited by design (default 127.0.0.1:7879)",
      },
      {
        name: "cinnabar mushlings {init|verify} [PATH]",
        description:
          "Exercises that teach the language through its own diagnostics; the real compiler decides whether a fix is right",
      },
      {
        name: "cinnabar fuzz replay <FILE>",
        description:
          "Recompile a saved fuzz artifact and report whether it still reproduces its failure",
      },
      {
        name: "cinnabar fuzz minimize <FILE> [-o FILE]",
        description:
          "Shrink an artifact to the smallest source with the same failure signature",
      },
      {
        name: "cinnabar native-stub <IDL> -o <FILE>",
        description:
          "Generate a typed, opaque nat type / nat fun surface from the constrained native IDL",
      },
    ],
  },
] as const;

/** How `cinnabar test` decides what is expected of a file, from its name. */
export const TEST_LAYOUT: readonly Row[] = [
  { name: "case.cnb", description: "Must compile, link, and exit 0" },
  {
    name: "case.cnb.exit",
    description: "The non-zero status case.cnb is expected to exit with",
  },
  {
    name: "case.reject.cnb",
    description: "Must be rejected; compiling it successfully is a failure",
  },
  {
    name: "case.reject.cnb.stderr",
    description: "The exact diagnostic that rejection must produce",
  },
] as const;

/** Local test-profile overrides. The full profile ignores all of these. */
export const TEST_ENV: readonly Row[] = [
  {
    name: "CINNABAR_FUZZ_POSITIVE_CASES",
    description: "Generated valid programs compiled",
  },
  {
    name: "CINNABAR_FUZZ_NEGATIVE_CASES",
    description: "Generated invalid linearity programs rejected",
  },
  {
    name: "CINNABAR_FUZZ_RUN_CASES",
    description: "Valid fuzz programs additionally linked and executed",
  },
  {
    name: "CINNABAR_REPRO_RUN_CASES",
    description: "Expected-success fixtures additionally linked and executed",
  },
  {
    name: "CINNABAR_REPRO_RECORD_CASES",
    description: "Record-only fixtures compiled and run",
  },
  {
    name: "CINNABAR_REPRO_LINK_COMPILE_ONLY",
    description:
      "Whether blocking compile-only fixtures are linked (true) instead of stopping at LLVM IR (false)",
  },
  {
    name: "CINNABAR_TEST_RUN_TIMEOUT_SECS",
    description: "Per-program execution timeout",
  },
  {
    name: "CINNABAR_TEST_COMPILE_TIMEOUT_SECS",
    description: "Per-program fuzz compilation timeout",
  },
] as const;

export type Profile = {
  name: string;
  corpus: string;
  nativeFuzz: string;
  nativeFixtures: string;
  recordOnly: string;
};

export const TEST_PROFILES: readonly Profile[] = [
  {
    name: "full",
    corpus: "80 valid + 80 invalid",
    nativeFuzz: "all 80 valid cases",
    nativeFixtures: "all",
    recordOnly: "all",
  },
  {
    name: "balanced",
    corpus: "32 valid + 32 invalid",
    nativeFuzz: "8",
    nativeFixtures: "10",
    recordOnly: "2",
  },
  {
    name: "smoke",
    corpus: "8 valid + 8 invalid",
    nativeFuzz: "2",
    nativeFixtures: "4",
    recordOnly: "0",
  },
] as const;
