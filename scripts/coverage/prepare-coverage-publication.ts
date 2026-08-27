import { coveragePaths } from "./coverage-paths";
import {
  renderCoveragePdfs,
  type RenderedCoveragePdfs,
  type RenderCoveragePdfsOptions,
} from "./render-coverage-pdf";
import { coverageUpdatedAt, renderCoverageReport } from "./render-coverage-report";

/** Files and timestamp prepared immediately before coverage publication. */
export interface PreparedCoveragePublication {
  readonly html: {
    readonly overview: string;
    readonly rust: string;
    readonly typescript: string;
  };
  readonly pdf: RenderedCoveragePdfs;
  readonly updatedAt: string;
}

/** Optional collaborators for coverage publication preparation. */
export interface PrepareCoveragePublicationOptions {
  readonly renderPdfs?: (
    workspaceRoot?: string,
    options?: RenderCoveragePdfsOptions,
  ) => Promise<RenderedCoveragePdfs>;
}

/**
 * Stamps every Cipher coverage page and renders matching PDFs.
 *
 * @param workspaceRoot - Cipher checkout containing both LCOV reports.
 * @param updatedAt - Project-owned publication time.
 * @returns Prepared HTML, PDF, and canonical timestamp metadata.
 */
export async function prepareCoveragePublication(
  workspaceRoot = process.cwd(),
  updatedAt = new Date().toISOString(),
  options: PrepareCoveragePublicationOptions = {},
): Promise<PreparedCoveragePublication> {
  const paths = coveragePaths(workspaceRoot);
  const publicationDate = coverageUpdatedAt(updatedAt);
  renderCoverageReport(paths.typescriptLcov, paths.rustLcov, paths.directory, publicationDate);
  const pdf = await (options.renderPdfs ?? renderCoveragePdfs)(workspaceRoot);

  console.log(`Prepared coverage publication: ${publicationDate}`);

  return {
    html: {
      overview: paths.overview.html,
      rust: paths.rust.html,
      typescript: paths.typescript.html,
    },
    pdf,
    updatedAt: publicationDate,
  };
}

/** Runs preparation and reports a non-sensitive CLI failure. */
export async function prepareCoveragePublicationCli(
  prepare?: () => Promise<unknown>,
  errorLog: (message: string) => void = console.error,
): Promise<boolean> {
  try {
    await (prepare ?? prepareCoveragePublication)();
    return true;
  } catch (error) {
    errorLog(error instanceof Error ? error.message : String(error));
    return false;
  }
}

if (import.meta.main) {
  if (!(await prepareCoveragePublicationCli())) process.exit(1);
}
