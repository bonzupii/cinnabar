import Window, { WindowBody } from "@/components/Window";
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
  cwd = "~/src/cinnabar",
  title = "Terminal session",
  className,
}: {
  lines: readonly ShellLine[];
  /** Shown beside the mark — the directory the session is running in. */
  cwd?: string;
  /** Centred: what the session demonstrates. */
  title?: string;
  className?: string;
}) {
  return (
    <Window path={cwd} title={title} className={className}>
      <WindowBody>
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
      </WindowBody>
    </Window>
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
    <Window path="cinnabar --help" title="Usage" className={className}>
      <WindowBody className="leading-[1.75]">
        <code>
          {lines.map((line, index) => (
            <span key={index}>
              <Tokens tokens={tokenizeUsageLine(line)} />
              {index < lines.length - 1 ? "\n" : null}
            </span>
          ))}
        </code>
      </WindowBody>
    </Window>
  );
}

/**
 * A block of plain text in a window — a config file, or a fenced block in a
 * repository document whose language the site has no theme for.
 */
export function PlainWindow({
  text,
  path,
  title = "Output",
  className,
}: {
  text: string;
  /** Shown beside the mark — a file name, or the language of the block. */
  path: string;
  /** Centred: what the block is showing. */
  title?: string;
  className?: string;
}) {
  return (
    <Window path={path} title={title} className={className}>
      <WindowBody className="text-term-output text-[12.5px] leading-[1.7]">
        <code>{text}</code>
      </WindowBody>
    </Window>
  );
}
