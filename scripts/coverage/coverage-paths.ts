import { resolve } from "node:path";

/** Fixed paths for Cipher's multi-page coverage publication. */
export interface CoveragePaths {
  readonly directory: string;
  readonly lcov: string;
  readonly overview: {
    readonly html: string;
    readonly pdf: string;
  };
  readonly typescript: {
    readonly html: string;
    readonly pdf: string;
  };
}

/**
 * Resolves the coverage pages beneath a workspace.
 *
 * @param workspaceRoot - Cipher checkout containing the coverage directory.
 * @returns Absolute LCOV, HTML, and PDF paths.
 */
export function coveragePaths(workspaceRoot = process.cwd()): CoveragePaths {
  const directory = resolve(workspaceRoot, "coverage");
  const typescriptDirectory = resolve(directory, "typescript");

  return {
    directory,
    lcov: resolve(directory, "lcov.info"),
    overview: {
      html: resolve(directory, "index.html"),
      pdf: resolve(directory, "index.pdf"),
    },
    typescript: {
      html: resolve(typescriptDirectory, "index.html"),
      pdf: resolve(typescriptDirectory, "index.pdf"),
    },
  };
}
