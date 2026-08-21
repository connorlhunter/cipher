import { expect, test } from "bun:test";

import {
  assertCoverageThreshold,
  coverageTotals,
  minimumCoveragePercent,
} from "../../scripts/coverage/assert-coverage";

const passingLcov = "SF:src/example.ts\nFNF:20\nFNH:19\nLF:20\nLH:19\nend_of_record\n";

test("enforces a global 95 percent line and function coverage floor", () => {
  expect(minimumCoveragePercent).toBe(95);
  expect(coverageTotals(passingLcov)).toEqual({
    functions: { covered: 19, found: 20 },
    lines: { covered: 19, found: 20 },
  });
  expect(assertCoverageThreshold(passingLcov, "TypeScript")).toEqual({
    functions: { covered: 19, found: 20 },
    lines: { covered: 19, found: 20 },
  });
});

test("rejects either global metric below the 95 percent floor", () => {
  expect(() =>
    assertCoverageThreshold(
      "SF:src/example.ts\nFNF:20\nFNH:18\nLF:20\nLH:19\nend_of_record\n",
      "Rust",
    ),
  ).toThrow("Rust coverage threshold failed: functions 90.00% < 95.00%");
});
