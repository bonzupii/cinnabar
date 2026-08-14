/** Every route in the static export, shared by the e2e specs. */
export const ROUTES = [
  // The home heading is the wordmark, whose accessible name is the project.
  { name: "home", path: "/", heading: /^Cinnabar$/, og: "/opengraph-image" },
  {
    name: "manifesto",
    path: "/manifesto/",
    heading: /The Cinnabar Manifesto/,
    og: "/manifesto/opengraph-image",
  },
  {
    name: "install",
    path: "/install/",
    heading: /Build the compiler/,
    og: "/install/opengraph-image",
  },
  {
    name: "reference",
    path: "/reference/",
    heading: /Two ways to invoke it/,
    og: "/reference/opengraph-image",
  },
  {
    name: "architecture",
    path: "/architecture/",
    heading: /One fixed pipeline/,
    og: "/architecture/opengraph-image",
  },
  {
    name: "roadmap",
    path: "/roadmap/",
    heading: /Resolved, and planned/,
    og: "/roadmap/opengraph-image",
  },
] as const;
