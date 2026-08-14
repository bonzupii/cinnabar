import type { ReactNode } from "react";

/*
 * Icon set — plate 07.
 *
 * 24 px grid, 1.6 px stroke, built from the square, the diamond, and lines at
 * 34°, 45° or 90°. No curves except the LSP dot. Vermilion marks the one part
 * of each icon that carries the meaning; nothing else in the set is coloured.
 *
 * Plate 07 also specifies the stroke compensation at the smallest step: the
 * 16 px rendering is drawn at 1.8 rather than 1.6.
 */

const ACCENT = "var(--cinnabar)";

type IconProps = {
  size?: number;
  className?: string;
  /** Accessible name. Omitted icons are hidden from assistive technology. */
  title?: string;
};

function Icon({
  size = 24,
  className,
  title,
  children,
}: IconProps & { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      // Plate 07: the stroke thickens at 16 px so the figure holds together.
      strokeWidth={size <= 16 ? 1.8 : 1.6}
      className={className}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
      style={{ display: "block", flex: "none" }}
    >
      {title ? <title>{title}</title> : null}
      {children}
    </svg>
  );
}

export function BuildIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polygon points="12,2.5 21.5,12 12,21.5 2.5,12" />
      <polyline points="8.5,11 12,14.5 15.5,11" stroke={ACCENT} />
    </Icon>
  );
}

export function RunIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polygon points="12,2.5 21.5,12 12,21.5 2.5,12" />
      <polygon points="10,8.5 16,12 10,15.5" fill={ACCENT} stroke="none" />
    </Icon>
  );
}

export function CheckIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polygon points="12,2.5 21.5,12 12,21.5 2.5,12" />
      <polyline points="8,12 11,15 16,9" stroke={ACCENT} />
    </Icon>
  );
}

export function BorrowIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polygon points="8,3.5 14.5,9.5 8,15.5 1.5,9.5" />
      <polygon points="16,8.5 22.5,14.5 16,20.5 9.5,14.5" stroke={ACCENT} />
    </Icon>
  );
}

export function LinearIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <line x1="12" y1="1.5" x2="12" y2="7" />
      <rect x="5.5" y="7" width="13" height="10" />
      <line x1="12" y1="17" x2="12" y2="22.5" stroke={ACCENT} />
    </Icon>
  );
}

export function LspIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polyline points="8,3.5 3,12 8,20.5" />
      <polyline points="16,3.5 21,12 16,20.5" />
      {/* The one curve the set allows. */}
      <circle cx="12" cy="12" r="1.9" fill={ACCENT} stroke="none" />
    </Icon>
  );
}

export function FmtIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <line x1="3" y1="6.5" x2="21" y2="6.5" />
      <line x1="3" y1="12" x2="14.5" y2="12" />
      <line x1="3" y1="17.5" x2="18" y2="17.5" stroke={ACCENT} />
    </Icon>
  );
}

export function DocIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="3" width="13" height="18" />
      <line x1="20" y1="6.5" x2="20" y2="21" stroke={ACCENT} />
    </Icon>
  );
}

export function TestIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polyline points="4,12 7,15.5 11.5,8" />
      <polyline points="12.5,12 15.5,15.5 20,8" stroke={ACCENT} />
    </Icon>
  );
}

export function CodegenIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polygon points="12,2.5 19,9.5 12,16.5 5,9.5" />
      <line x1="5.5" y1="20.5" x2="11" y2="20.5" />
      <line x1="13" y1="20.5" x2="18.5" y2="20.5" stroke={ACCENT} />
    </Icon>
  );
}

export function DiagnosticIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <polygon points="12,2.5 21.5,12 12,21.5 2.5,12" />
      <line x1="12" y1="7.5" x2="12" y2="13.5" stroke={ACCENT} />
      <circle cx="12" cy="16.6" r="1.05" fill={ACCENT} stroke="none" />
    </Icon>
  );
}

export function StaticLinkIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3" y="3" width="18" height="18" />
      <line x1="3" y1="14.5" x2="21" y2="14.5" />
      <rect x="15" y="16.6" width="4" height="2.4" fill={ACCENT} stroke="none" />
    </Icon>
  );
}

/**
 * GitHub's own glyph. Not part of the brand set — it is a third-party mark and
 * deliberately drawn with its real curves rather than forced onto the 34°
 * grid, which would misrepresent someone else's logo.
 */
export function GitHubIcon({ size = 16, className, title }: IconProps) {
  return (
    <svg
      viewBox="0 0 16 16"
      width={size}
      height={size}
      fill="currentColor"
      className={className}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
      style={{ display: "block", flex: "none" }}
    >
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

/** The pipeline stage each icon stands for, used by the architecture page. */
export const PIPELINE_ICONS = {
  build: BuildIcon,
  run: RunIcon,
  check: CheckIcon,
  borrow: BorrowIcon,
  linear: LinearIcon,
  lsp: LspIcon,
  fmt: FmtIcon,
  doc: DocIcon,
  test: TestIcon,
  codegen: CodegenIcon,
  diagnostic: DiagnosticIcon,
  staticLink: StaticLinkIcon,
} as const;
