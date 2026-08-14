import { readFile } from "node:fs/promises";
import path from "node:path";
import GithubSlugger from "github-slugger";

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
 *
 * A block that carries a repeated thing — a highlight, a shipped capability —
 * is divided again by `###` headings, and `items` returns those as a list:
 *
 *     <!-- @highlights -->
 *
 *     ### No lifetime annotations
 *
 *     Borrow scopes are flow-sensitive and inferred by the compiler.
 *
 * Each item is keyed by the slug of its heading, which is what binds it to the
 * structure that stays in TypeScript — the icon it is drawn with, the anchor
 * it links to, the order it appears in. Rewording a heading therefore breaks
 * the build rather than silently pairing the wrong icon with the new title.
 */

const BLOCK_MARKER = /^[ \t]*<!--\s*@([\w-]+)\s*-->[ \t]*$/;
const ITEM_HEADING = /^###\s+(.+?)\s*$/;
const LIST_ITEM = /^[-*]\s+(.+?)\s*$/;

/** One `###` section of a block: its heading, its slug, and its prose. */
export type ContentItem = { slug: string; title: string; body: string };

export type PageContent = {
  /** Returns a block, or throws naming the file and key if it is missing. */
  block(name: string): string;
  /** Returns a block, or undefined when it is absent. */
  optional(name: string): string | undefined;
  /** A block's `###` sections, in file order. Throws if the block is empty. */
  items(name: string): ContentItem[];
  /** One `###` section by slug. Throws naming the slug if it is absent. */
  item(name: string, slug: string): ContentItem;
  /** A block's top-level bullets, as plain strings. */
  list(name: string): string[];
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

/**
 * Splits one block into its `###` sections.
 *
 * Exported for testing. Text before the first heading is dropped: a block used
 * this way is a list of things, and a preamble would have nowhere to render.
 */
export function parseContentItems(source: string): ContentItem[] {
  const slugger = new GithubSlugger();
  const items: ContentItem[] = [];
  let current: { title: string; slug: string } | undefined;
  let buffer: string[] = [];

  const flush = () => {
    if (current) {
      items.push({ ...current, body: buffer.join("\n").trim() });
    }
    buffer = [];
  };

  for (const line of source.split(/\r?\n/)) {
    const heading = ITEM_HEADING.exec(line);
    if (heading) {
      flush();
      current = { title: heading[1], slug: slugger.slug(heading[1]) };
      continue;
    }
    buffer.push(line);
  }
  flush();

  return items;
}

/** Pulls the top-level bullets out of a block, without their markers. */
export function parseContentList(source: string): string[] {
  return source
    .split(/\r?\n/)
    .map((line) => LIST_ITEM.exec(line)?.[1])
    .filter((item): item is string => item !== undefined);
}

/** Reads and parses `src/app/<route>/content.md`. */
export async function readPageContent(route: string): Promise<PageContent> {
  const file = path.join(process.cwd(), "src", "app", route, "content.md");
  const blocks = parseContentBlocks(await readFile(file, "utf8"));

  const block = (name: string): string => {
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
  };

  const items = (name: string): ContentItem[] => {
    const parsed = parseContentItems(block(name));
    if (parsed.length === 0) {
      throw new Error(
        `content.md for route "${route}": the @${name} block has no "### " items`,
      );
    }
    return parsed;
  };

  return {
    block,
    optional: (name) => blocks[name],
    items,
    item(name, slug) {
      const found = items(name).find((entry) => entry.slug === slug);
      if (!found) {
        throw new Error(
          `content.md for route "${route}": the @${name} block has no item "${slug}" (found: ${items(
            name,
          )
            .map((entry) => entry.slug)
            .join(", ")})`,
        );
      }
      return found;
    },
    list: (name) => parseContentList(block(name)),
    names: Object.keys(blocks),
  };
}
