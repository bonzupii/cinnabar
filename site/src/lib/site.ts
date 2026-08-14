/*
 * Single source of truth for the site's identity strings and navigation.
 *
 * The taglines and the positioning sentence are the brand board's own copy
 * (plates 00, 11 and 12) — they are not rewritten here.
 */

export const REPO_URL = "https://github.com/bonzupii/cinnabar";

/**
 * The deployed origin, used as the metadata base for canonical and social
 * URLs.
 *
 * Netlify exposes the production origin as URL during a build, so the deployed
 * site picks it up without anything being hard-coded. A local build, a
 * Playwright run and a Lighthouse run all fall back to the port the test
 * server uses, which keeps canonical and og:image URLs resolvable there too.
 */
export const SITE_URL =
  process.env.NEXT_PUBLIC_SITE_URL ?? process.env.URL ?? "http://localhost:4173";


/** Plate 00 — the cover's positioning sentence. */
export const TAGLINE =
  "A statically-typed systems language with Austral-style linear typing. No garbage collector. No lifetime annotations. No reachable panics.";

/** Plate 00 and 11 — the line the dev put on the repo. */
export const QUIP = "Probably better than Rust.";

/** Plate 11 — the social banner's longer description. */
export const DESCRIPTION =
  "A systems language where every resource is consumed exactly once — checked at compile time, with no lifetime annotations and no garbage collector.";

/** Plate 12 — the metadata strip. */
export const BADGES = ["Apache-2.0", "LLVM 21", "musl · static"] as const;
export const STATUS_BADGE = "early development";

export type NavItem = {
  href: string;
  label: string;
  /** Shown in the mobile menu under the label. */
  blurb: string;
  /** The plate 07 icon that stands for this section, used wherever it appears. */
  icon: IconName;
};

/**
 * Icons are named rather than imported here, because this module is read by
 * the sitemap and the metadata — neither of which should pull in components.
 */
export type IconName = "doc" | "build" | "reference" | "architecture" | "check";

/**
 * Plate 12's docs header sets the order: the normative spec first, then the
 * practical surfaces, then the plan.
 */
export const NAV: readonly NavItem[] = [
  {
    href: "/manifesto/",
    label: "Manifesto",
    blurb: "The normative language specification.",
    icon: "doc",
  },
  {
    href: "/install/",
    label: "Install",
    blurb: "Build the compiler and set up an editor.",
    icon: "build",
  },
  {
    href: "/reference/",
    label: "Reference",
    blurb: "Every CLI flag, command and manifest field.",
    icon: "reference",
  },
  {
    href: "/architecture/",
    label: "Architecture",
    blurb: "The seven pipeline stages, end to end.",
    icon: "architecture",
  },
  {
    href: "/roadmap/",
    label: "Roadmap",
    blurb: "What is resolved and what is planned.",
    icon: "check",
  },
] as const;

/** Every route the export produces, for the sitemap and the link checker. */
export const ROUTES = ["/", ...NAV.map((item) => item.href)] as const;

/** Marks the active nav item, treating nested routes as part of their family. */
export function isActiveRoute(pathname: string, href: string): boolean {
  const normalise = (value: string) =>
    value.endsWith("/") && value !== "/" ? value.slice(0, -1) : value;
  const current = normalise(pathname);
  const target = normalise(href);
  if (target === "/") return current === "/";
  return current === target || current.startsWith(`${target}/`);
}
