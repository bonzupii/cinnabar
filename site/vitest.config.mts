import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    // Playwright owns tests/e2e; vitest owns the pure-function unit tests.
    include: ["tests/unit/**/*.test.ts"],
    environment: "node",
  },
});
