/*
 * The CLI surface, transcribed from README.md.
 *
 * There are two invocation forms: given a source file, `cinnabar` runs the
 * whole pipeline and writes a static binary; given a subcommand, it acts on
 * the project whose build.cnb manifest is found by walking upward from the
 * supplied path.
 *
 * The flags, the commands and everything written about them are prose, and
 * live in src/app/reference/content.md. Each section here names a set of
 * blocks by its id — `<id>-heading`, `<id>-note`, an optional `<id>-intro`,
 * and `<id>-rows`, whose `###` sections are the table's rows: the heading is
 * the flag or command, and the paragraph under it is what it does. Keeping the
 * two halves of a row in one place is the point — a name and its description
 * cannot drift apart when they are the same paragraph.
 *
 * What stays here is what a table cannot be edited into: the usage synopsis,
 * which is a literal transcription of what `--help` prints, the order and
 * grouping of the sections, and the profile matrix below.
 */

export type Row = { name: string; description: string };

export type Section = {
  /** Anchors the section, and names its blocks in reference/content.md. */
  id: string;
  /** The heading of the table's first column. */
  nameHeading: string;
};

export const USAGE = `cinnabar <FILE> [-o|--output PATH] [--dump-ast] [--dump-typed-ast] [--print-layout]
                [--emit-llvm] [--emit-obj] [--explain-borrow[=human|json]] [--run]
                [-O|--opt-level {0,1,2,3,s,z}]
cinnabar <COMMAND> [ARGS]`;

export const CLI_SECTIONS: readonly Section[] = [
  { id: "single-file", nameHeading: "Flag" },
  { id: "project", nameHeading: "Command" },
  { id: "inspect", nameHeading: "Command" },
] as const;

export type Profile = {
  name: string;
  corpus: string;
  nativeFuzz: string;
  nativeFixtures: string;
  recordOnly: string;
};

/**
 * The local test profiles.
 *
 * A matrix of counts rather than prose: every cell is a budget, no cell is a
 * sentence, and the five columns only line up when they are read as a table.
 * Moving this to markdown would replace five aligned fields with a paragraph
 * per profile, which is harder to edit and harder to check, so it stays.
 */
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
