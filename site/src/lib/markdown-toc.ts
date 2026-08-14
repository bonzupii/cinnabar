import GithubSlugger from "github-slugger";

/*
 * Builds a table of contents from a markdown document.
 *
 * The slugs must match the ids `rehype-slug` puts on the rendered headings, or
 * every link in the contents is broken. rehype-slug uses github-slugger, so
 * this uses the same library rather than a hand-rolled slug function — and the
 * same slugger instance across the document, because its duplicate-suffixing
 * (`-1`, `-2`) is stateful and a fresh instance per heading would drift.
 */

export type TocEntry = {
  depth: number;
  text: string;
  slug: string;
};

/** ``code`` spans, **emphasis** and links are stripped to their text. */
function stripInlineMarkdown(text: string): string {
  return text
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\*\*([^*]*)\*\*/g, "$1")
    .replace(/\*([^*]*)\*/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .trim();
}

/**
 * Extracts headings between `minDepth` and `maxDepth`.
 *
 * Fenced code is skipped: a `# comment` inside a Cinnabar or shell block is
 * not a heading, and several of these documents contain exactly that.
 */
export function extractHeadings(
  markdown: string,
  { minDepth = 2, maxDepth = 3 }: { minDepth?: number; maxDepth?: number } = {},
): TocEntry[] {
  const slugger = new GithubSlugger();
  const entries: TocEntry[] = [];
  let inFence = false;
  let fence = "";

  for (const line of markdown.split(/\r?\n/)) {
    const fenceMatch = /^\s*(```+|~~~+)/.exec(line);
    if (fenceMatch) {
      if (!inFence) {
        inFence = true;
        fence = fenceMatch[1][0];
      } else if (fenceMatch[1][0] === fence) {
        inFence = false;
      }
      continue;
    }
    if (inFence) continue;

    const heading = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (!heading) continue;

    const depth = heading[1].length;
    const text = stripInlineMarkdown(heading[2]);
    if (text.length === 0) continue;

    // Every heading is slugged, even one outside the requested range, so the
    // duplicate counter stays in step with what rehype-slug produced.
    const slug = slugger.slug(text);
    if (depth < minDepth || depth > maxDepth) continue;

    entries.push({ depth, text, slug });
  }

  return entries;
}
