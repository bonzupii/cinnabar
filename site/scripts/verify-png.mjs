/*
 * Asserts that each argument is a real PNG of the expected size.
 *
 * Two things are checked with it:
 *
 * - The social images, which are produced by a route handler rather than by
 *   Next's `opengraph-image` convention, because that convention emits an
 *   extension-less file and a static host then serves it without an image
 *   content-type.
 * - The raster brand assets in public/brand, written by
 *   scripts/generate-brand-assets.tsx.
 *
 * Either way the failure this exists to catch is the same: an empty file, an
 * HTML error page or a wrong canvas reaching a README or a preview card. It
 * should fail the build instead.
 *
 * Usage:
 *   verify-png.mjs <file...>                 # expects the 1200x630 card
 *   verify-png.mjs --size 512x512 <file...>  # expects that size from here on
 *
 * `--size` applies to every file after it, so one invocation can check
 * several different canvases.
 */
import { readFile } from "node:fs/promises";

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

/** The social card, which is what this checked before it took a --size flag. */
const DEFAULT_SIZE = { width: 1200, height: 630 };

const args = process.argv.slice(2);

if (args.length === 0) {
  console.error("usage: verify-png.mjs [--size WxH] <file...>");
  process.exit(1);
}

/** Parses a `WxH` argument, or exits saying what was wrong with it. */
function parseSize(value) {
  const match = /^(\d+)x(\d+)$/.exec(value ?? "");
  if (!match) {
    console.error(`--size expects WxH, got ${value ?? "nothing"}`);
    process.exit(1);
  }
  return { width: Number(match[1]), height: Number(match[2]) };
}

let expected = DEFAULT_SIZE;
let failed = false;
let checked = 0;

for (let index = 0; index < args.length; index += 1) {
  if (args[index] === "--size") {
    expected = parseSize(args[index + 1]);
    index += 1;
    continue;
  }

  const file = args[index];
  checked += 1;

  try {
    const data = await readFile(file);

    if (!data.subarray(0, 8).equals(PNG_SIGNATURE)) {
      console.error(`FAIL ${file}: not a PNG (signature mismatch)`);
      failed = true;
      continue;
    }

    // IHDR is the first chunk; width and height are big-endian u32 at 16 and 20.
    const width = data.readUInt32BE(16);
    const height = data.readUInt32BE(20);

    if (width !== expected.width || height !== expected.height) {
      console.error(
        `FAIL ${file}: expected ${expected.width}x${expected.height}, got ${width}x${height}`,
      );
      failed = true;
      continue;
    }

    console.log(`ok   ${file} — ${width}x${height}, ${data.length} bytes`);
  } catch (error) {
    console.error(`FAIL ${file}: ${error instanceof Error ? error.message : error}`);
    failed = true;
  }
}

if (checked === 0) {
  console.error("no files given");
  process.exit(1);
}

process.exit(failed ? 1 : 0);
