/** Every route in the static export, shared by the e2e specs. */
export const ROUTES = [
  // The home heading is the wordmark, whose accessible name is the project.
  { name: "home", path: "/", heading: /^Cinnabar$/, og: "/og-image" },
  {
    name: "manifesto",
    path: "/manifesto/",
    heading: /The Cinnabar Manifesto/,
    og: "/manifesto/og-image",
  },
  {
    name: "install",
    path: "/install/",
    heading: /Build the compiler/,
    og: "/install/og-image",
  },
  {
    name: "reference",
    path: "/reference/",
    heading: /Two ways to invoke it/,
    og: "/reference/og-image",
  },
  {
    name: "architecture",
    path: "/architecture/",
    heading: /One fixed pipeline/,
    og: "/architecture/og-image",
  },
  {
    name: "roadmap",
    path: "/roadmap/",
    heading: /Eight milestones down/,
    og: "/roadmap/og-image",
  },
] as const;
