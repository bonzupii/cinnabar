import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App.jsx";

// The brand's two faces, self-hosted rather than fetched.
//
// `@fontsource` ships the woff2 files as package contents, so Vite emits
// them into `dist/assets` and the page loads them from its own origin. That
// is not a preference: the playground container runs with `network_mode:
// none`, and a webfont pulled from a CDN would simply never arrive.
//
// Only the weights the interface actually sets are imported. Schibsted
// Grotesk carries the chrome at 400/500/600; IBM Plex Mono carries code,
// labels and tables at 400/500.
import "@fontsource/schibsted-grotesk/400.css";
import "@fontsource/schibsted-grotesk/500.css";
import "@fontsource/schibsted-grotesk/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";

import "./styles.css";

createRoot(document.getElementById("root")).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
