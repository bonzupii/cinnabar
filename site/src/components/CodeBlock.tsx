import Window, { WindowBody } from "@/components/Window";
import { tokenizeCinnabar, type TokenKind } from "@/lib/cinnabar-syntax";

/*
 * Renders Cinnabar source in the "Cinnabar Dark" theme — plate 09.
 *
 * "Only keywords take the accent, so the eye reads control flow first. Linear
 * handles get a dotted underline instead of a colour: the one thing the
 * language cares about is marked structurally, not chromatically."
 *
 * The code surface keeps its own dark ground in both site themes. The theme is
 * specified against that ground, and plate 14's last misuse rule forbids
 * adding colours to it, so there is no light variant to invent.
 */

const TOKEN_STYLE: Record<TokenKind, string> = {
  keyword: "text-syn-keyword",
  type: "text-syn-type font-medium",
  constant: "text-syn-type",
  function: "text-syn-type font-semibold",
  identifier: "text-syn-identifier",
  literal: "text-syn-literal",
  string: "text-syn-literal",
  punctuation: "text-syn-punctuation",
  comment: "text-syn-comment italic",
  "doc-comment": "text-syn-comment italic",
  text: "",
};

export default function CodeBlock({
  code,
  linearHandles,
  path = "source.cnb",
  title = "Cinnabar source",
  className,
}: {
  code: string;
  /**
   * Bindings the typechecker would mark linear. Plate 09 gives these a dotted
   * rule; the highlighter cannot infer linearity, so callers state it.
   */
  linearHandles?: readonly string[];
  /** Shown beside the mark — the file this source came from. */
  path?: string;
  /** Centred: what the sample demonstrates. */
  title?: string;
  className?: string;
}) {
  const tokens = tokenizeCinnabar(code.replace(/\n+$/, ""), linearHandles);

  return (
    <Window path={path} title={title} className={className}>
      <WindowBody tone="code" scale="source">
        <code>
          {tokens.map((token, position) => {
            if (token.kind === "text") {
              return <span key={position}>{token.value}</span>;
            }
            return (
              <span
                key={position}
                className={`${TOKEN_STYLE[token.kind]}${token.linear ? " linear-handle" : ""}`}
              >
                {token.value}
              </span>
            );
          })}
        </code>
      </WindowBody>
    </Window>
  );
}
