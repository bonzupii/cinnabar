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


/**
 * The site's meta description, and the sentence the hero opens with.
 *
 * Plate 00's cover line is "A statically-typed systems language with
 * Austral-style linear typing. No garbage collector. No lifetime annotations.
 * No reachable panics." The three-part "No X. No Y. No Z." is a cadence rather
 * than a fact, and a developer on the project rejected copy of that shape. The
 * claims are kept and the cadence is not: the same sentence now names the
 * checker that enforces them and quotes README.md's own blunt closing line.
 *
 * Kept in step with the `@tagline` block of src/app/(home)/content.md, which
 * is the rendered copy; this is the string the <meta> tag needs, where
 * markdown would appear verbatim.
 */
export const TAGLINE =
  "A statically-typed systems language with Austral-style linear types, checked by a flow-sensitive borrow checker. There is no #[allow], and no flag that turns a check off.";

/** Plate 00 and 11 — the line the dev put on the repo. */
export const QUIP = "Probably better than Rust.";

/**
 * The site's social description.
 *
 * Plate 11's own banner line is "A systems language where every resource is
 * consumed exactly once — checked at compile time, with no lifetime
 * annotations and no garbage collector." It is deliberately not used here.
 * "Consumed exactly once" is a property of linear types, and repeating it as
 * the description of the whole project made every preview card read as a
 * slogan. It now appears once, where linear typing is actually explained.
 */
export const DESCRIPTION =
  "A statically-typed systems language for compilers, runtimes, kernels, firmware and network stacks. Resource handles are linear and borrow-checked without lifetime annotations, there is no garbage collector, and there is no #[allow] to switch a check off.";

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
export type IconName = "doc" | "build" | "reference" | "architecture" | "check" | "playground";

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
    href: "/playground/",
    label: "Playground",
    blurb: "Type Cinnabar, checked in your browser.",
    icon: "playground",
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
