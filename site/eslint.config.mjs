import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";
import betterTailwind from "eslint-plugin-better-tailwindcss";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  /*
   * Canonical Tailwind classes, enforced rather than suggested.
   *
   * An arbitrary value that a registered token already produces —
   * `bg-[color:var(--hairline-strong)]` for `bg-hairline-strong`,
   * `tracking-[-0.025em]` for `tracking-tight` — kept coming back: the editor
   * reported it, someone fixed it, and the next rewrite of the component
   * reintroduced it. A suggestion nobody's build reads is not a rule.
   *
   * `enforce-canonical-classes` is the plugin's implementation of Tailwind's
   * own canonical suggestions, which is the same mechanism the editor was
   * reporting from, so the two now agree. It resolves tokens from the theme by
   * reading globals.css, which means a token added there is covered without
   * this file changing.
   *
   * `collapse` is off. It rewrites four sides into a shorthand
   * (`mt-2 mr-2 mb-2 ml-2` into `m-2`), which is a different judgement from
   * "you spelled a token the long way" and would churn layout classes that are
   * written per side on purpose.
   */
  {
    files: ["src/**/*.{ts,tsx}", "scripts/**/*.tsx", "tests/**/*.tsx"],
    plugins: { "better-tailwindcss": betterTailwind },
    settings: {
      "better-tailwindcss": { entryPoint: "src/app/globals.css" },
    },
    rules: {
      "better-tailwindcss/enforce-canonical-classes": [
        "error",
        { collapse: false },
      ],
    },
  },
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
