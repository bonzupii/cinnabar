import { parseFlow } from "@/lib/ascii-diagram";

/*
 * A figure a repository document drew in text.
 *
 * The documents draw the compiler pipeline as an untagged fenced block of box
 * characters. Rendered through the shared window chrome it read as a terminal
 * transcript titled "output" — the wrong thing entirely, since nothing printed
 * it. This presents it as what it is: a figure, centred, on the page's own
 * ground, with no chrome.
 *
 * A plain top-to-bottom flow is redrawn — hairline boxes and vermilion arrows,
 * the board's own devices — from the labels parsed out of the block. Anything
 * else keeps the author's drawing verbatim in mono. The choice is made from
 * the characters, never from the wording, so an edit upstream changes what the
 * figure says rather than whether it appears.
 */

/** The drawn form: one box per label, an arrow between each pair. */
function Flow({ nodes }: { nodes: readonly string[] }) {
  return (
    <ol className="flex list-none flex-col items-center gap-0">
      {nodes.map((node, index) => (
        <li key={`${node}-${index}`} className="flex flex-col items-center">
          {index > 0 ? (
            /*
              The connector: a hairline stem with a vermilion head, which is
              the one place the accent appears in the figure. Hidden from
              assistive technology — the list already carries the order, and
              announcing "down arrow" between every pair says nothing the
              numbering has not.
            */
            <span
              aria-hidden="true"
              className="flex flex-col items-center py-1.5"
            >
              <span className="bg-hairline-strong h-5 w-px" />
              <span className="text-cinnabar-text -mt-1 font-mono text-[13px] leading-none">
                ▼
              </span>
            </span>
          ) : null}
          <span className="border-hairline-strong bg-panel text-text inline-block border px-6 py-3 text-center font-mono text-[13px] tracking-[0.02em] wrap-break-word sm:text-[14px]">
            {node}
          </span>
        </li>
      ))}
    </ol>
  );
}

/** The undrawable form: the author's own art, centred and given room. */
function AsArt({ text }: { text: string }) {
  return (
    <pre
      tabIndex={0}
      className="text-secondary w-full overflow-x-auto text-center font-mono text-[12.5px] leading-[1.6]"
    >
      <code>{text}</code>
    </pre>
  );
}

export default function AsciiDiagram({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  const nodes = parseFlow(text);
  return (
    <figure
      className={`border-hairline bg-ground flex flex-col items-center gap-7 border px-6 py-12 sm:px-10 sm:py-14 ${
        className ?? ""
      }`}
    >
      {nodes ? <Flow nodes={nodes} /> : <AsArt text={text} />}
      <figcaption className="text-label font-mono text-[10px] tracking-[0.16em] uppercase">
        Diagram
      </figcaption>
    </figure>
  );
}
