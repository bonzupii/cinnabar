import Markdown from "@/components/Markdown";
import TableOfContents from "@/components/TableOfContents";
import { extractHeadings } from "@/lib/markdown-toc";

/*
 * The layout for a long repository document.
 *
 * These files run to hundreds of lines — the manifesto and the roadmap are
 * both book-length by web standards — and as a single column they read as a
 * wall. A contents rail beside the prose turns the same text into something
 * navigable, and gives the reader a sense of how much is left.
 *
 * The rail only appears where there is room for it; below the large breakpoint
 * the document is a single column, since a sticky sidebar on a phone would
 * cost more space than it earns.
 */
export default function DocBody({
  markdown,
  tocLabel,
}: {
  markdown: string;
  tocLabel?: string;
}) {
  const headings = extractHeadings(markdown, { minDepth: 2, maxDepth: 2 });

  return (
    <div className="mx-auto grid max-w-[1400px] gap-x-16 gap-y-10 px-6 sm:px-10 lg:grid-cols-[minmax(0,1fr)_240px]">
      {/*
        The prose comes first in the source so it is what a screen reader and a
        keyboard reach first; the rail is placed to its right visually.
      */}
      <div className="min-w-0 lg:order-1">
        <Markdown>{markdown}</Markdown>
      </div>
      <aside className="hidden min-w-0 lg:order-2 lg:block">
        <TableOfContents entries={headings} label={tocLabel} />
      </aside>
    </div>
  );
}
