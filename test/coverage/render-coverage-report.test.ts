import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "bun:test";
import {
  coverageUpdatedAt,
  parseLcov,
  renderCoverageReport,
} from "../../scripts/coverage/render-coverage-report";

let directory = "";

afterEach(() => {
  if (directory) rmSync(directory, { force: true, recursive: true });
  directory = "";
});

test("renders labeled coverage pages from Bun LCOV output", () => {
  directory = mkdtempSync(join(tmpdir(), "cipher-coverage-"));
  const typeScriptLcovPath = join(directory, "typescript.lcov");
  const rustLcovPath = join(directory, "rust.lcov");
  writeFileSync(typeScriptLcovPath, "SF:src/desktop.ts\nFNF:2\nFNH:1\nLF:4\nLH:3\nend_of_record\n");
  writeFileSync(
    rustLcovPath,
    `SF:${join(process.cwd(), "apps/server.rs")}\nFNF:1\nFNH:1\nLF:2\nLH:1\nend_of_record\n`,
  );

  expect(parseLcov(readFileSync(typeScriptLcovPath, "utf8"))).toEqual([
    {
      functions: { covered: 1, found: 2 },
      lines: { covered: 3, found: 4 },
      path: "src/desktop.ts",
    },
  ]);

  renderCoverageReport(
    typeScriptLcovPath,
    rustLcovPath,
    join(directory, "coverage"),
    "2026-08-20T14:42:31.123-04:00",
  );
  const index = readFileSync(join(directory, "coverage", "index.html"), "utf8");
  const rust = readFileSync(join(directory, "coverage", "rust", "index.html"), "utf8");
  const typeScript = readFileSync(join(directory, "coverage", "typescript", "index.html"), "utf8");
  expect(index).toContain("TypeScript");
  expect(index).toContain('href="rust/index.html"');
  expect(index).toContain('Updated <time datetime="2026-08-20T18:42:31.123Z">Aug 20, 2026</time>');
  expect(index).toContain("connorhunter.theme.scheme");
  expect(index).toContain('aria-current="page"');
  expect(typeScript).toContain("75.00% (3/4)");
  expect(typeScript).toContain(
    'Updated <time datetime="2026-08-20T18:42:31.123Z">Aug 20, 2026</time>',
  );
  expect(typeScript).toContain('href="../index.html"');
  expect(rust).toContain("50.00% (1/2)");
  expect(rust).toContain("Generated with cargo-llvm-cov.");
  expect(rust).toContain("apps/server.rs");
  expect(rust).not.toContain(process.cwd());
});

test("normalizes the project-owned coverage timestamp", () => {
  expect(coverageUpdatedAt("2026-08-20T14:42:31.123-04:00")).toBe("2026-08-20T18:42:31.123Z");
  expect(() => coverageUpdatedAt("not-a-date")).toThrow("Invalid coverage publication date");
});
