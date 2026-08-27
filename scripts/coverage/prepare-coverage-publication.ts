import { rmSync } from "node:fs";
import { join } from "node:path";
import { coveragePaths } from "./coverage-paths";
import { renderCoveragePdfs } from "./render-coverage-pdf";
import { coverageUpdatedAt, renderCoverageReport } from "./render-coverage-report";

export interface PreparedCoveragePublication {
  readonly json: string;
  readonly pdf: string;
  readonly updatedAt: string;
}

/** Builds Cipher's timestamped JSON/PDF coverage pair before publishing. */
export async function prepareCoveragePublication(
  workspaceRoot = process.cwd(),
  updatedAt = new Date().toISOString(),
): Promise<PreparedCoveragePublication> {
  const paths = coveragePaths(workspaceRoot);
  const publicationDate = coverageUpdatedAt(updatedAt);
  clearRetiredCoverageOutput(paths.directory);
  const json = renderCoverageReport(
    paths.typescriptLcov,
    paths.rustLcov,
    paths.directory,
    publicationDate,
  );
  const { overview: pdf } = await renderCoveragePdfs(workspaceRoot);
  return { json, pdf, updatedAt: publicationDate };
}

/** Removes retired standalone coverage pages before syncing the reader artifacts. */
function clearRetiredCoverageOutput(directory: string): void {
  for (const path of [
    join(directory, "index.html"),
    join(directory, "index.pdf"),
    join(directory, "rust"),
    join(directory, "typescript"),
  ]) {
    rmSync(path, { force: true, recursive: true });
  }
}

if (import.meta.main) {
  try {
    await prepareCoveragePublication();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
