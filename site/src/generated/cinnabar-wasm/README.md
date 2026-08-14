# Generated, plus one required patch

`cinnabar_wasm.js` and `cinnabar_wasm.d.ts` are `wasm-bindgen`'s JS glue for
`crates/cinnabar-wasm`, built for the `web` target. The `.wasm` binary itself
lives at `site/public/wasm/cinnabar_wasm_bg.wasm` instead of alongside these
—`init()` is called with that explicit path (see `PlaygroundEditor.tsx`)
rather than the default `import.meta.url`-relative lookup, so the asset has a
stable path under Next's `output: "export"` static build regardless of
bundler-specific `new URL()` handling.

Regenerate from the repo root (inside `nix develop`, which provides a
`wasm-bindgen-cli` pinned to the version `crates/cinnabar-wasm/Cargo.toml`
requires):

```bash
cargo build -p cinnabar-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir /tmp/cinnabar-wasm-pkg --out-name cinnabar_wasm \
  target/wasm32-unknown-unknown/release/cinnabar_wasm.wasm
cp /tmp/cinnabar-wasm-pkg/cinnabar_wasm.js /tmp/cinnabar-wasm-pkg/cinnabar_wasm.d.ts site/src/generated/cinnabar-wasm/
cp /tmp/cinnabar-wasm-pkg/cinnabar_wasm_bg.wasm site/public/wasm/
```

Netlify's build does not regenerate this — there's no Rust toolchain in that
build image — so a change to `crates/cinnabar-wasm` (or to any pipeline stage
it calls into) needs the commands above run and the diff committed alongside
it.

## The required patch

`wasm-bindgen`'s `web` target always emits a fallback in its `init()` that
resolves the `.wasm` relative to the JS file when no path is given:

```js
if (module_or_path === undefined) {
    module_or_path = new URL('cinnabar_wasm_bg.wasm', import.meta.url);
}
```

Next's bundler statically resolves that `new URL(..., import.meta.url)`
expression at build time — regardless of whether the branch ever runs — and
fails the build with `Module not found` once the `.wasm` no longer sits next
to this file. Every caller here always passes an explicit path, so that
branch is genuinely unreachable; after each regeneration, replace it with:

```js
if (module_or_path === undefined) {
    throw new Error('cinnabar_wasm: call default(url) with an explicit path to the .wasm binary');
}
```
