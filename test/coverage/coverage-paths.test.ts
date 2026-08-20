import { join } from "node:path";
import { expect, test } from "bun:test";
import { coveragePaths } from "../../scripts/coverage/coverage-paths";

test("resolves overview and TypeScript coverage files", () => {
  expect(coveragePaths("/workspace/cipher")).toEqual({
    directory: join("/workspace/cipher", "coverage"),
    lcov: join("/workspace/cipher", "coverage", "lcov.info"),
    overview: {
      html: join("/workspace/cipher", "coverage", "index.html"),
      pdf: join("/workspace/cipher", "coverage", "index.pdf"),
    },
    typescript: {
      html: join("/workspace/cipher", "coverage", "typescript", "index.html"),
      pdf: join("/workspace/cipher", "coverage", "typescript", "index.pdf"),
    },
  });
});
