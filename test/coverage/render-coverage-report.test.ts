import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "bun:test";
import { parseLcov, renderCoverageReport } from "../../scripts/coverage/render-coverage-report";

let directory = "";

afterEach(() => {
  if (directory) rmSync(directory, { force: true, recursive: true });
  directory = "";
});

test("renders labeled coverage pages from Bun LCOV output", () => {
  directory = mkdtempSync(join(tmpdir(), "cipher-coverage-"));
  const lcovPath = join(directory, "lcov.info");
  writeFileSync(lcovPath, "SF:src/desktop.ts\nFNF:2\nFNH:1\nLF:4\nLH:3\nend_of_record\n");

  expect(parseLcov(readFileSync(lcovPath, "utf8"))).toEqual([
    {
      functions: { covered: 1, found: 2 },
      lines: { covered: 3, found: 4 },
      path: "src/desktop.ts",
    },
  ]);

  renderCoverageReport(lcovPath, join(directory, "coverage"));
  const index = readFileSync(join(directory, "coverage", "index.html"), "utf8");
  const typeScript = readFileSync(join(directory, "coverage", "typescript", "index.html"), "utf8");
  expect(index).toContain("TypeScript");
  expect(index).toContain("connorhunter.theme.scheme");
  expect(index).toContain('aria-current="page"');
  expect(typeScript).toContain("75.00% (3/4)");
  expect(typeScript).toContain('href="../index.html"');
});
