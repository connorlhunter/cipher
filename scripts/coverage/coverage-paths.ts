import { resolve } from "node:path";

/** Fixed coverage inputs and published artifacts beneath a Cipher checkout. */
export interface CoveragePaths {
  readonly directory: string;
  readonly json: string;
  readonly pdf: string;
  readonly rustLcov: string;
  readonly typescriptLcov: string;
}

/** Resolves the coverage inputs and JSON/PDF publication pair. */
export function coveragePaths(workspaceRoot = process.cwd()): CoveragePaths {
  const directory = resolve(workspaceRoot, "coverage");
  return {
    directory,
    json: resolve(directory, "index.json"),
    pdf: resolve(directory, "coverage.pdf"),
    rustLcov: resolve(directory, "rust.lcov"),
    typescriptLcov: resolve(directory, "lcov.info"),
  };
}
