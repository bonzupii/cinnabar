<!-- @lede -->

Given a source file, `cinnabar` runs the whole pipeline and writes a static
binary. Given a subcommand, it acts on the project whose `build.cnb` manifest is
discovered by walking upward from the supplied path.

<!-- @manifest -->

`build.cnb` is Cinnabar source, not a configuration format. It is read back
through the compiler's own front end, so it obeys the same casing, typing and
literal rules as any other program.

`NAME` names the built artifact and must be a single path component. `ENTRY` and
`TESTS` are relative paths confined to the project root. `TESTS` may be omitted,
and then defaults to `tests`.

`build` and `run` name the artifact after the manifest's `NAME` rather than after
whichever file happens to be the entry — a project that renames its entry source
has not renamed itself.

<!-- @test-layout -->

A `.stderr` sidecar makes its test a rejection test whether or not the name says
`.reject`, and the snapshot is compared in full rather than searched for a
substring — a diagnostic is part of what the compiler promises, so a change to
its wording is a change to be reviewed. `--update-snapshots` is for deliberately
accepting a diagnostic whose diff you have read, not for making a red run go
green.

<!-- @profiles -->

Individual budgets can be overridden when a reduced profile is still broader or
narrower than needed. The full profile ignores these variables, so an exported
local override cannot silently reduce the gate's coverage.

<!-- @self-documenting -->

Each command is documented in the binary itself — `cinnabar <COMMAND> --help`
prints the full description, not a one-line summary.
