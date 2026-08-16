// The brand palette, as the site defines it.
//
// These are not new colours. Every value here is copied from
// `site/src/app/globals.css`, which in turn carries them from the brand
// board (`.planning/brand/designs/Cinnabar Brand.dc.html`): one accent,
// five greys, and plate 09's "Cinnabar Dark" code theme.
//
// A copy that could drift is checked against its original rather than
// trusted, which is what `test/brand.drift.test.js` does — it parses the
// site's stylesheet and requires every value below to match. Adding a
// colour here that the site does not define fails that test, which is the
// point: plate 14's misuse rules forbid extending the theme.

/**
 * The surface and text ramp, from the site's dark `:root`.
 *
 * The playground is a code surface end to end, and the site's own rule for
 * code surfaces is that they do not follow the light/dark preference —
 * plate 09 is specified against the dark ground and plate 05 says the
 * screen system stays dark. So this tool is dark in both, exactly like the
 * `<pre>` blocks on the site are.
 */
export const SURFACE = {
  ground: "#100e0d",
  panel: "#171514",
  panelRaised: "#242120",
  hairline: "#302c2a",
  hairlineStrong: "#423d3a",
  mute: "#6e6763",
  grey: "#7c7570",
  secondary: "#a29b96",
  bright: "#c9c2bd",
  text: "#ede9e6",
  label: "#928a85",
  cinnabar: "#e0442a",
  cinnabarDeep: "#a82d1b",
  cinnabarText: "#f26a4f",
  onCinnabar: "#100e0d",
};

/** Plate 09, "Cinnabar Dark": six roles, and no seventh. */
export const SYNTAX = {
  ground: "#100e0d",
  terminal: "#0b0a09",
  keyword: "#e0442a",
  type: "#ede9e6",
  identifier: "#c9c2bd",
  literal: "#a29b96",
  punctuation: "#9a928d",
  comment: "#8a837e",
};

/** Plate 10's diagnostic roles, as the site's terminal transcripts use them. */
export const TERMINAL = {
  prompt: "#928a85",
  command: "#ede9e6",
  flag: "#c9c2bd",
  output: "#a29b96",
  error: "#e0442a",
  gutter: "#928a85",
};
