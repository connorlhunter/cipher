import { coveragePaths } from "./coverage-paths";
import { renderCoveragePdfs, type RenderedCoveragePdfs } from "./render-coverage-pdf";
import { coverageUpdatedAt, renderCoverageReport } from "./render-coverage-report";

/** Files and timestamp prepared immediately before coverage publication. */
export interface PreparedCoveragePublication {
  readonly html: {
    readonly overview: string;
    readonly typescript: string;
  };
  readonly pdf: RenderedCoveragePdfs;
  readonly updatedAt: string;
}

/**
 * Stamps every Cipher coverage page and renders matching PDFs.
 *
 * @param workspaceRoot - Cipher checkout containing the LCOV report.
 * @param updatedAt - Project-owned publication time.
 * @returns Prepared HTML, PDF, and canonical timestamp metadata.
 */
export async function prepareCoveragePublication(
  workspaceRoot = process.cwd(),
  updatedAt = new Date().toISOString(),
): Promise<PreparedCoveragePublication> {
  const paths = coveragePaths(workspaceRoot);
  const publicationDate = coverageUpdatedAt(updatedAt);
  renderCoverageReport(paths.lcov, paths.directory, publicationDate);
  const pdf = await renderCoveragePdfs(workspaceRoot);

  console.log(`Prepared coverage publication: ${publicationDate}`);

  return {
    html: { overview: paths.overview.html, typescript: paths.typescript.html },
    pdf,
    updatedAt: publicationDate,
  };
}

if (import.meta.main) {
  try {
    await prepareCoveragePublication();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
