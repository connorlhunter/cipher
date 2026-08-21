import { readFileSync } from "node:fs";

import { parseLcov } from "./render-coverage-report";

export const minimumCoveragePercent = 95;

interface CoverageMetric {
  readonly covered: number;
  readonly found: number;
}

export interface CoverageTotals {
  readonly functions: CoverageMetric;
  readonly lines: CoverageMetric;
}

/** Calculates global line and function coverage from one LCOV report. */
export function coverageTotals(lcov: string): CoverageTotals {
  return parseLcov(lcov).reduce<CoverageTotals>(
    (totals, file) => ({
      functions: {
        covered: totals.functions.covered + file.functions.covered,
        found: totals.functions.found + file.functions.found,
      },
      lines: {
        covered: totals.lines.covered + file.lines.covered,
        found: totals.lines.found + file.lines.found,
      },
    }),
    { functions: { covered: 0, found: 0 }, lines: { covered: 0, found: 0 } },
  );
}

function percentage(metric: CoverageMetric): number {
  return metric.found === 0 ? 100 : (metric.covered / metric.found) * 100;
}

/** Throws unless global line and function coverage meet the configured floor. */
export function assertCoverageThreshold(
  lcov: string,
  surface: string,
  threshold = minimumCoveragePercent,
): CoverageTotals {
  const totals = coverageTotals(lcov);
  const failures = (Object.entries(totals) as Array<[keyof CoverageTotals, CoverageMetric]>)
    .filter(([, metric]) => percentage(metric) < threshold)
    .map(
      ([metric, value]) => `${metric} ${percentage(value).toFixed(2)}% < ${threshold.toFixed(2)}%`,
    );

  if (failures.length > 0) {
    throw new Error(`${surface} coverage threshold failed: ${failures.join(", ")}`);
  }
  return totals;
}

/** Reads and validates a persisted LCOV report. */
export function assertCoverageFile(path: string, surface: string): CoverageTotals {
  return assertCoverageThreshold(readFileSync(path, "utf8"), surface);
}
