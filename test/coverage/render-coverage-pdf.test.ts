import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test } from "bun:test";
import { renderCoveragePdfs } from "../../scripts/coverage/render-coverage-pdf";

describe("renderCoveragePdfs", () => {
  let directory = "";

  afterEach(() => {
    if (directory) rmSync(directory, { force: true, recursive: true });
    directory = "";
  });

  test("renders both coverage pages as PDFs", async () => {
    directory = mkdtempSync(join(tmpdir(), "cipher-coverage-pdf-"));
    const coverage = join(directory, "coverage");
    const typescript = join(coverage, "typescript");
    mkdirSync(typescript, { recursive: true });
    writeFileSync(join(coverage, "index.html"), "<!doctype html><h1>Cipher coverage</h1>");
    writeFileSync(join(typescript, "index.html"), "<!doctype html><h1>TypeScript coverage</h1>");

    const output = await renderCoveragePdfs(directory);

    expect(output).toEqual({
      overview: join(coverage, "index.pdf"),
      typescript: join(typescript, "index.pdf"),
    });
    for (const path of Object.values(output)) {
      expect(existsSync(path)).toBe(true);
      expect(readFileSync(path).subarray(0, 4).toString()).toBe("%PDF");
    }
  });

  test("requires both HTML pages", async () => {
    directory = mkdtempSync(join(tmpdir(), "cipher-coverage-pdf-"));

    await expect(renderCoveragePdfs(directory)).rejects.toThrow("Missing coverage report");
  });
});
