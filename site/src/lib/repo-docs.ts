import { readFile } from "node:fs/promises";
import path from "node:path";

/*
 * The manifesto, roadmap and architecture pages are the repository's own
 * markdown, read at build time from the checkout one level above this app.
 *
 * They are not copied into the site. A copy would drift, and MANIFESTO.md is
 * normative — a stale copy of a normative spec is worse than no copy. The
 * build fails loudly if a document is missing rather than rendering an empty
 * page.
 */

const REPO_ROOT = path.join(process.cwd(), "..");

export const REPO_URL = "https://github.com/bonzupii/cinnabar";
const BLOB_BASE = `${REPO_URL}/blob/main/`;
const TREE_BASE = `${REPO_URL}/tree/main/`;

/** Repository documents that have a page of their own on this site. */
export const DOC_ROUTES: Record<string, string> = {
  "MANIFESTO.md": "/manifesto/",
  "ROADMAP.md": "/roadmap/",
  "ARCHITECTURE.md": "/architecture/",
};

export type RepoDocName =
  | "MANIFESTO.md"
  | "ROADMAP.md"
  | "ARCHITECTURE.md"
  | "AGENTS.md"
  | "CONTAINER_DEVELOPMENT.md";

/**
 * Rewrites the repo-relative links in a markdown document so they resolve from
 * the site.
 *
 * A document that has its own page here links to that page; everything else —
 * source files, fixtures, directories — links into GitHub, because there is
 * nothing on this site to point at. Absolute URLs and in-page anchors are left
 * alone.
 */
export function rewriteRepoLinks(markdown: string): string {
  return markdown.replace(
    /\]\((?!https?:\/\/|#|mailto:)([^)\s]+)(\s+"[^"]*")?\)/g,
    (_match, target: string, title: string | undefined) => {
      const [pathPart, anchor = ""] = target.split("#") as [string, string?];
      const clean = pathPart.replace(/^\.\//, "");
      const suffix = anchor ? `#${anchor}` : "";
      const titlePart = title ?? "";

      const ownPage = DOC_ROUTES[clean];
      if (ownPage) return `](${ownPage}${suffix}${titlePart})`;

      // A trailing slash means a directory, which GitHub serves under /tree.
      const base = clean.endsWith("/") ? TREE_BASE : BLOB_BASE;
      return `](${base}${clean}${suffix}${titlePart})`;
    },
  );
}

/**
 * Drops the document's leading H1. Each page renders its own header from the
 * brand's type scale, so keeping the source H1 would show the title twice and
 * put two `<h1>`s on the page.
 */
export function stripLeadingHeading(markdown: string): string {
  return markdown.replace(/^#\s+.*(\r?\n)+/, "");
}

/** Reads a repository document and prepares it for rendering on the site. */
export async function readRepoDoc(name: RepoDocName): Promise<string> {
  const raw = await readFile(path.join(REPO_ROOT, name), "utf8");
  return rewriteRepoLinks(stripLeadingHeading(raw));
}
