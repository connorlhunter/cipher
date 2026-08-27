import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";

export interface CoverageMetric {
  readonly covered: number;
  readonly found: number;
}

export interface CoverageFile {
  readonly functions: CoverageMetric;
  readonly lines: CoverageMetric;
  readonly path: string;
}

export interface CoverageSurface {
  readonly files: ReadonlyArray<CoverageFile>;
  readonly id: string;
  readonly label: string;
  readonly totals: CoverageFile;
}

export interface CoverageArtifact {
  readonly minimumCoverage: { readonly functions: number; readonly lines: number };
  readonly schemaVersion: 2;
  readonly surfaces: ReadonlyArray<CoverageSurface>;
  readonly updatedAt: string;
}

const minimumCoverage = { functions: 95, lines: 95 } as const;

/** Parses an LCOV report into the file metrics used by the portfolio reader. */
export function parseLcov(lcov: string): CoverageFile[] {
  const files: CoverageFile[] = [];
  let current: CoverageFile | undefined;

  for (const line of lcov.split(/\r?\n/u)) {
    if (line.startsWith("SF:")) {
      current = {
        functions: { covered: 0, found: 0 },
        lines: { covered: 0, found: 0 },
        path: safePath(line.slice(3)),
      };
      continue;
    }
    if (!current) continue;
    if (line.startsWith("FNF:")) current = setMetric(current, "functions", "found", value(line));
    if (line.startsWith("FNH:")) current = setMetric(current, "functions", "covered", value(line));
    if (line.startsWith("LF:")) current = setMetric(current, "lines", "found", value(line));
    if (line.startsWith("LH:")) current = setMetric(current, "lines", "covered", value(line));
    if (line === "end_of_record") {
      files.push(current);
      current = undefined;
    }
  }

  return files;
}

/** Normalizes a publication time to ISO UTC. */
export function coverageUpdatedAt(value: string): string {
  const timestamp = new Date(value);
  if (Number.isNaN(timestamp.getTime()))
    throw new Error(`Invalid coverage publication date: ${value}`);
  return timestamp.toISOString();
}

/** Builds Cipher's single structured coverage artifact. */
export function coverageArtifact(
  typescriptFiles: ReadonlyArray<CoverageFile>,
  rustFiles: ReadonlyArray<CoverageFile>,
  updatedAt: string,
): CoverageArtifact {
  return {
    minimumCoverage,
    schemaVersion: 2,
    surfaces: [
      surface("typescript", "TypeScript", typescriptFiles),
      surface("rust", "Rust", rustFiles),
    ],
    updatedAt: coverageUpdatedAt(updatedAt),
  };
}

/** Writes JSON coverage data. */
export function renderCoverageReport(
  typescriptLcovPath = join("coverage", "lcov.info"),
  rustLcovPath = join("coverage", "rust.lcov"),
  outputRoot = "coverage",
  updatedAt = new Date().toISOString(),
): string {
  const artifact = coverageArtifact(
    parseLcov(readFileSync(typescriptLcovPath, "utf8")),
    parseLcov(readFileSync(rustLcovPath, "utf8")),
    updatedAt,
  );
  mkdirSync(outputRoot, { recursive: true });
  const output = join(outputRoot, "index.json");
  writeFileSync(output, `${JSON.stringify(artifact, null, 2)}\n`);
  console.log(`Rendered coverage artifact: ${output}`);
  return output;
}

function value(line: string): number {
  return Number(line.split(":")[1] ?? 0);
}

function safePath(path: string): string {
  return path.startsWith("/") ? relative(process.cwd(), path) : path;
}

function setMetric(
  file: CoverageFile,
  metric: "functions" | "lines",
  field: "covered" | "found",
  value: number,
): CoverageFile {
  return { ...file, [metric]: { ...file[metric], [field]: value } };
}

function surface(id: string, label: string, files: ReadonlyArray<CoverageFile>): CoverageSurface {
  const sorted = [...files].sort((left, right) => left.path.localeCompare(right.path));
  return { files: sorted, id, label, totals: totals(sorted) };
}

function totals(files: ReadonlyArray<CoverageFile>): CoverageFile {
  return files.reduce<CoverageFile>(
    (total, file) => ({
      functions: metricTotal(total.functions, file.functions),
      lines: metricTotal(total.lines, file.lines),
      path: "All files",
    }),
    {
      functions: { covered: 0, found: 0 },
      lines: { covered: 0, found: 0 },
      path: "All files",
    },
  );
}

function metricTotal(left: CoverageMetric, right: CoverageMetric): CoverageMetric {
  return { covered: left.covered + right.covered, found: left.found + right.found };
}

if (import.meta.main) renderCoverageReport();
