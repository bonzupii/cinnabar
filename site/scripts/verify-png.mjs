/*
 * Asserts that each argument is a real PNG of the expected size.
 *
 * The social images are produced by a route handler rather than Next's
 * `opengraph-image` convention, because that convention emits an
 * extension-less file and GitHub Pages then serves it without an image
 * content-type. This check exists so that a regression in that setup — an
 * empty file, an HTML error page, a wrong canvas — fails the build instead of
 * shipping a broken preview card.
 */
import { readFile } from "node:fs/promises";

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

const expected = { width: 1200, height: 630 };
const files = process.argv.slice(2);

if (files.length === 0) {
  console.error("usage: verify-png.mjs <file...>");
  process.exit(1);
}

let failed = false;

for (const file of files) {
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

process.exit(failed ? 1 : 0);
