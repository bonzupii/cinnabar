import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Overrides the default ignores of eslint-config-next, which is why the
  // defaults have to be repeated here rather than added to.
  globalIgnores([
    // Defaults of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // Local Netlify state and build cache, written by `netlify link` and
    // `netlify deploy`. It contains a copy of the minified build output.
    ".netlify/**",
    // Brand design boards — hand-authored HTML, not part of the app.
    ".planning/**",
    // Tooling output.
    "playwright-report/**",
    "test-results/**",
    "blob-report/**",
    ".lighthouseci/**",
    "capture/**",
  ]),
]);

export default eslintConfig;
