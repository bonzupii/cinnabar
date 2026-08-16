/** Every route in the static export, shared by the e2e specs. */
export const ROUTES = [
  { name: "home", path: "/", heading: /^Cinnabar$/, og: "/og-image", nav: false },
  { name: "playground", path: "/playground/", heading: /Checked as you type/i, og: "/playground/og-image", nav: true },
  { name: "learn", path: "/learn/", heading: /From design stance to first program/, og: "/learn/og-image", nav: true },
  { name: "why cinnabar", path: "/learn/why-cinnabar/", heading: /Why Cinnabar/, og: "/learn/why-cinnabar/og-image", nav: false },
  { name: "linear types", path: "/learn/linear-types/", heading: /Linear types/, og: "/learn/linear-types/og-image", nav: false },
  { name: "borrowing", path: "/learn/borrowing/", heading: /Borrowing without lifetime syntax/, og: "/learn/borrowing/og-image", nav: false },
  { name: "error handling", path: "/learn/error-handling/", heading: /Failure stays explicit/, og: "/learn/error-handling/og-image", nav: false },
  { name: "first program", path: "/learn/first-program/", heading: /Your first Cinnabar program/, og: "/learn/first-program/og-image", nav: false },
  { name: "install", path: "/install/", heading: /Build the compiler/, og: "/install/og-image", nav: true },
  { name: "cli reference", path: "/reference/", heading: /Two ways to invoke it/, og: "/reference/og-image", nav: true },
  { name: "architecture", path: "/architecture/", heading: /One fixed pipeline/, og: "/architecture/og-image", nav: true },
  { name: "manifesto", path: "/manifesto/", heading: /The Cinnabar Manifesto/, og: "/manifesto/og-image", nav: false },
  { name: "roadmap", path: "/roadmap/", heading: /milestones/i, og: "/roadmap/og-image", nav: true },
  { name: "contributing", path: "/contributing/development/", heading: /Develop across worktrees/, og: "/contributing/development/og-image", nav: false },
] as const;

export const NAV_ROUTES = ROUTES.filter((route) => route.nav);
