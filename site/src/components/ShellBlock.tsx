import {
  tokenizeShellLine,
  tokenizeUsageLine,
  type ShellToken,
} from "@/lib/shell-syntax";

/*
 * Terminal transcripts and usage synopses, styled to plate 10.
 *
 * The command reads brightest, flags a step below it, the prompt sits in the
 * gutter grey, and program output stays quiet. Vermilion appears only on an
 * error — the board is explicit that there is no warning colour, because the
 * language has no warnings.
 */

const TOKEN_STYLE: Record<ShellToken["kind"], string> = {
  prompt: "text-term-prompt",
  command: "text-term-command",
  flag: "text-term-flag",
  placeholder: "text-term-flag",
  operator: "text-syn-punctuation",
  comment: "text-syn-comment italic",
  plain: "text-term-output",
};

function Tokens({ tokens }: { tokens: ShellToken[] }) {
  return (
    <>
      {tokens.map((token, index) => (
        <span key={index} className={TOKEN_STYLE[token.kind]}>
          {token.value}
        </span>
      ))}
    </>
  );
}

/** One line of a transcript: a command, or a line the program printed back. */
export type ShellLine = string | { out: string };

export default function ShellBlock({
  lines,
  caption,
  className,
}: {
  lines: readonly ShellLine[];
  caption?: string;
  className?: string;
}) {
  return (
    <figure className={`rule-grid flex min-w-0 flex-col ${className ?? ""}`}>
      {caption ? (
        <figcaption className="bg-panel text-label px-5 py-3 font-mono text-[11px] tracking-[0.14em] uppercase">
          {caption}
        </figcaption>
      ) : null}
      <pre
        tabIndex={0}
        className="bg-code-terminal w-full overflow-x-auto px-6 py-6 font-mono text-[13px] leading-[1.85] sm:text-[14px]"
      >
        <code>
          {lines.map((line, index) => {
            const isCommand = typeof line === "string";
            const text = isCommand ? line : line.out;
            return (
              <span key={index}>
                {isCommand ? <span className="text-term-prompt">$ </span> : null}
                <Tokens tokens={tokenizeShellLine(text, isCommand)} />
                {index < lines.length - 1 ? "\n" : null}
              </span>
            );
          })}
        </code>
      </pre>
    </figure>
  );
}

/** The synopsis shape `cinnabar --help` prints. */
export function UsageBlock({
  lines,
  className,
}: {
  lines: readonly string[];
  className?: string;
}) {
  return (
    <div className={`rule-grid flex min-w-0 ${className ?? ""}`}>
      <pre
        tabIndex={0}
        className="bg-code-terminal w-full overflow-x-auto px-6 py-6 font-mono text-[13px] leading-[1.75]"
      >
        <code>
          {lines.map((line, index) => (
            <span key={index}>
              <Tokens tokens={tokenizeUsageLine(line)} />
              {index < lines.length - 1 ? "\n" : null}
            </span>
          ))}
        </code>
      </pre>
    </div>
  );
}
