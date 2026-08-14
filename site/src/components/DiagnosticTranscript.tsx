import Window, { WindowBody } from "@/components/Window";
import {
  BORROW_DIAGNOSTIC,
  DIAGNOSTIC_LEGEND,
  type DiagnosticRole,
  type Segment,
} from "@/content/diagnostic";

/*
 * Renders a role-tagged diagnostic in plate 10's palette.
 *
 * The role-to-style table below is the whole styling contract: vermilion is
 * reserved for the error and its primary span, and everything else stays grey.
 * There is no warning role, because the language has no warnings.
 */
const ROLE_STYLE: Record<DiagnosticRole, string> = {
  error: "text-term-error font-semibold",
  message: "text-term-command font-semibold",
  source: "text-term-flag",
  secondary: "text-term-output",
  gutter: "text-term-gutter",
  prompt: "text-term-prompt",
  command: "text-term-command",
  flag: "text-term-flag",
};

export default function DiagnosticTranscript({
  lines = BORROW_DIAGNOSTIC,
  className,
}: {
  lines?: readonly Segment[][];
  className?: string;
}) {
  return (
    <Window path="~/src/kernel" title="Borrow diagnostic" className={className}>
      <WindowBody className="text-[12.5px] leading-[1.7] sm:text-[13.5px]">
        <code>
          {lines.map((segments, index) => (
            <span key={index}>
              {segments.map((segment, position) => (
                <span key={position} className={ROLE_STYLE[segment.role]}>
                  {segment.text}
                </span>
              ))}
              {index < lines.length - 1 ? "\n" : null}
            </span>
          ))}
        </code>
      </WindowBody>
    </Window>
  );
}

/**
 * The legend beside the transcript.
 *
 * Plate 09 puts a filled swatch next to each label rather than tinting the
 * label itself — which is also the only honest way to show these values, since
 * setting "#7C7570" in #7C7570 would not be legible on this panel.
 */
export function DiagnosticLegend() {
  return (
    <dl className="border-hairline flex flex-col border-t">
      {DIAGNOSTIC_LEGEND.map(({ role, value, weight }) => (
        <div
          key={role}
          className="border-hairline flex items-center gap-3.5 border-b py-3.5 font-mono text-xs"
        >
          <span
            aria-hidden="true"
            className="border-hairline h-3.5 w-3.5 flex-none border"
            style={{ background: value }}
          />
          <dt className="text-label">{role}</dt>
          <dd className="text-secondary ml-auto">
            {value} · {weight}
          </dd>
        </div>
      ))}
    </dl>
  );
}
