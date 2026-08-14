import { tokenizeCinnabar, type TokenKind } from "@/lib/cinnabar-syntax";

/*
 * Renders Cinnabar source in the "Cinnabar Dark" theme — plate 09.
 *
 * "Only keywords take the accent, so the eye reads control flow first. Linear
 * handles get a dotted underline instead of a colour: the one thing the
 * language cares about is marked structurally, not chromatically."
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

type CodeBlockProps = {
  code: string;
  /**
   * Bindings the typechecker would mark linear. Plate 09 gives these a dotted
   * rule; the highlighter cannot infer linearity, so callers state it.
   */
  linearHandles?: readonly string[];
  /** Shown as a mono caption above the block — conventionally the file path. */
  caption?: string;
  className?: string;
};

export default function CodeBlock({
  code,
  linearHandles,
  caption,
  className,
}: CodeBlockProps) {
  const tokens = tokenizeCinnabar(code.replace(/\n+$/, ""), linearHandles);

  return (
    <figure className={`rule-grid flex min-w-0 flex-col ${className ?? ""}`}>
      {caption ? (
        <figcaption className="bg-panel px-5 py-3 font-mono text-[11px] tracking-[0.14em] text-label uppercase">
          {caption}
        </figcaption>
      ) : null}
      <pre tabIndex={0} className="bg-code-ground overflow-x-auto px-6 py-6 font-mono text-[13.5px] leading-[1.75] sm:text-[15px]">
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
      </pre>
    </figure>
  );
}

/**
 * A terminal transcript in the diagnostic palette from plate 10. Diagnostics
 * are pre-styled text rather than tokenized source: vermilion is reserved for
 * the error and its primary span, and everything else stays grey. There is no
 * warning colour, because the language has no warnings.
 */
export function TerminalBlock({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={`rule-grid flex min-w-0 ${className ?? ""}`}>
      <pre tabIndex={0} className="bg-code-terminal text-term-output w-full overflow-x-auto px-6 py-7 font-mono text-[12.5px] leading-[1.7] sm:text-[14px]">
        {children}
      </pre>
    </div>
  );
}
