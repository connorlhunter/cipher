import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "bun:test";
import { coveragePaths } from "../../scripts/coverage/coverage-paths";
import {
  coverageArtifact,
  parseLcov,
  renderCoverageReport,
} from "../../scripts/coverage/render-coverage-report";
import { renderCoveragePdfs } from "../../scripts/coverage/render-coverage-pdf";

let workspaceRoot = "";

afterEach(() => {
  if (workspaceRoot) rmSync(workspaceRoot, { force: true, recursive: true });
  workspaceRoot = "";
});

test("writes one structured coverage artifact and PDF for TypeScript and Rust", async () => {
  const lcov = "SF:src/example.ts\nFNF:1\nFNH:1\nLF:2\nLH:2\nend_of_record\n";
  expect(
    coverageArtifact(parseLcov(lcov), parseLcov(lcov), "2026-08-20T18:42:31.123Z"),
  ).toMatchObject({
    minimumCoverage: { functions: 95, lines: 95 },
    schemaVersion: 2,
    surfaces: [{ id: "typescript" }, { id: "rust" }],
  });

  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-coverage-artifact-"));
  const paths = coveragePaths(workspaceRoot);
  mkdirSync(paths.directory, { recursive: true });
  writeFileSync(paths.typescriptLcov, lcov);
  writeFileSync(paths.rustLcov, lcov);
  renderCoverageReport(
    paths.typescriptLcov,
    paths.rustLcov,
    paths.directory,
    "2026-08-20T18:42:31.123Z",
  );

  expect(JSON.parse(readFileSync(paths.json, "utf8"))).toMatchObject({
    updatedAt: "2026-08-20T18:42:31.123Z",
  });

  await renderCoveragePdfs(workspaceRoot);
  expect(existsSync(paths.pdf)).toBe(true);
  expect(statSync(paths.pdf).size).toBeGreaterThan(0);
});
