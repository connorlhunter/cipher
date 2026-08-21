import { join } from "node:path";
import { expect, test } from "bun:test";
import { coveragePaths } from "../../scripts/coverage/coverage-paths";

test("resolves overview, TypeScript, and Rust coverage files", () => {
  expect(coveragePaths("/workspace/cipher")).toEqual({
    directory: join("/workspace/cipher", "coverage"),
    rustLcov: join("/workspace/cipher", "coverage", "rust.lcov"),
    typescriptLcov: join("/workspace/cipher", "coverage", "lcov.info"),
    overview: {
      html: join("/workspace/cipher", "coverage", "index.html"),
      pdf: join("/workspace/cipher", "coverage", "index.pdf"),
    },
    rust: {
      html: join("/workspace/cipher", "coverage", "rust", "index.html"),
      pdf: join("/workspace/cipher", "coverage", "rust", "index.pdf"),
    },
    typescript: {
      html: join("/workspace/cipher", "coverage", "typescript", "index.html"),
      pdf: join("/workspace/cipher", "coverage", "typescript", "index.pdf"),
    },
  });
});
