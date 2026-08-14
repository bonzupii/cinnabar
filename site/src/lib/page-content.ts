import { readFile } from "node:fs/promises";
import path from "node:path";

/*
 * Per-route editable copy.
 *
 * Every route keeps its prose in a `content.md` beside its `page.tsx`, so
 * changing wording never means reading JSX. Next only treats `page`, `route`,
 * `layout` and friends as routable, so a `.md` sitting in the app directory is
 * inert.
 *
 * A page is rarely one continuous document — it has a lede, a couple of
 * section intros, a closing note — so the file is divided into named blocks:
 *
 *     <!-- @lede -->
 *     Markdown for the lede.
 *
 *     <!-- @closing -->
 *     Markdown for the closing section.
 *
 * Text before the first marker is the `body` block, which is all a
 * prose-only page needs.
 */

const BLOCK_MARKER = /^[ \t]*<!--\s*@([\w-]+)\s*-->[ \t]*$/;

export type PageContent = {
  /** Returns a block, or throws naming the file and key if it is missing. */
  block(name: string): string;
  /** Returns a block, or undefined when it is absent. */
  optional(name: string): string | undefined;
  /** Every block name present, in file order. */
  names: string[];
};

/**
 * Splits a content document into its named blocks.
 *
 * Exported for testing; `readPageContent` is the normal entry point.
 */
export function parseContentBlocks(source: string): Record<string, string> {
  const blocks: Record<string, string> = {};
  let current = "body";
  let buffer: string[] = [];

  const flush = () => {
    const text = buffer.join("\n").trim();
    if (text.length > 0) blocks[current] = text;
    buffer = [];
  };

  for (const line of source.split(/\r?\n/)) {
    const marker = BLOCK_MARKER.exec(line);
    if (marker) {
      flush();
      current = marker[1];
      continue;
    }
    buffer.push(line);
  }
  flush();

  return blocks;
}

/** Reads and parses `src/app/<route>/content.md`. */
export async function readPageContent(route: string): Promise<PageContent> {
  const file = path.join(process.cwd(), "src", "app", route, "content.md");
  const blocks = parseContentBlocks(await readFile(file, "utf8"));

  return {
    block(name) {
      const text = blocks[name];
      if (text === undefined) {
        // Failing the build beats rendering a page with a silently empty
        // section, which is easy to ship without noticing.
        throw new Error(
          `content.md for route "${route}" has no @${name} block (found: ${
            Object.keys(blocks).join(", ") || "none"
          })`,
        );
      }
      return text;
    },
    optional: (name) => blocks[name],
    names: Object.keys(blocks),
  };
}
