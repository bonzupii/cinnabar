import { renderToStaticMarkup } from "react-dom/server";
import Disclosure from "@/components/Disclosure";

/*
 * Prints the real Disclosure's server-rendered markup on stdout.
 *
 * This is not a spec — it is run as a subprocess by disclosure.spec.ts,
 * through `tsx`. It cannot be inlined into the spec: Playwright compiles JSX
 * (in the spec and in everything it imports) with its own component-testing
 * factory, which react-dom/server refuses to render. Running the render under
 * a normal React JSX transform is the only way to put the actual component's
 * output in front of a browser.
 *
 * Pass --open to render the `defaultOpen` variant.
 */

const html = renderToStaticMarkup(
  <Disclosure summary="Full grammar" defaultOpen={process.argv.includes("--open")}>
    <p>The grammar is in GRAMMAR.md.</p>
  </Disclosure>,
);

process.stdout.write(html);
