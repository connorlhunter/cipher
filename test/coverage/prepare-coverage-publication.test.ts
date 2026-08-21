import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, spyOn, test } from "bun:test";
import { coveragePaths } from "../../scripts/coverage/coverage-paths";
import { prepareCoveragePublication } from "../../scripts/coverage/prepare-coverage-publication";

const sampleLcov = "SF:src/example.ts\nFNF:1\nFNH:1\nLF:2\nLH:2\nend_of_record\n";

describe("prepareCoveragePublication", () => {
  let directory = "";

  afterEach(() => {
    if (directory) rmSync(directory, { force: true, recursive: true });
    directory = "";
  });

  test("uses one project timestamp for every page and PDF", async () => {
    spyOn(console, "log").mockImplementation(() => undefined);
    directory = mkdtempSync(join(tmpdir(), "cipher-coverage-publication-"));
    const coverage = join(directory, "coverage");
    mkdirSync(coverage, { recursive: true });
    writeFileSync(join(coverage, "lcov.info"), sampleLcov);
    writeFileSync(join(coverage, "rust.lcov"), sampleLcov);

    const result = await prepareCoveragePublication(directory, "2026-08-20T14:42:31.123-04:00", {
      renderPdfs: async (workspaceRoot) => {
        const paths = coveragePaths(workspaceRoot);
        for (const path of [paths.overview.html, paths.typescript.html, paths.rust.html]) {
          expect(readFileSync(path, "utf8")).toContain(
            'Updated <time datetime="2026-08-20T18:42:31.123Z">Aug 20, 2026</time>',
          );
        }
        for (const path of [paths.overview.pdf, paths.typescript.pdf, paths.rust.pdf]) {
          writeFileSync(path, "%PDF-1.4");
        }
        return {
          overview: paths.overview.pdf,
          rust: paths.rust.pdf,
          typescript: paths.typescript.pdf,
        };
      },
    });

    expect(result.updatedAt).toBe("2026-08-20T18:42:31.123Z");
    for (const path of Object.values(result.html)) {
      expect(readFileSync(path, "utf8")).toContain(
        'Updated <time datetime="2026-08-20T18:42:31.123Z">Aug 20, 2026</time>',
      );
    }
    for (const path of Object.values(result.pdf)) {
      expect(readFileSync(path).subarray(0, 4).toString()).toBe("%PDF");
    }
  });
});
