import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "bun:test";
import {
  assertCurrentRelease,
  buildChangelogArtifact,
  parseChangelog,
} from "../scripts/changelog/changelog-artifact";

let workspaceRoot = "";

afterEach(() => {
  if (workspaceRoot) rmSync(workspaceRoot, { force: true, recursive: true });
  workspaceRoot = "";
});

const changelog = `# Changelog

## [1.2.3] - 2026-08-27

### Added

- Publish the project changelog.
`;

test("builds Markdown and PDF artifacts from the canonical changelog", async () => {
  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-changelog-artifact-"));
  writeFileSync(join(workspaceRoot, "package.json"), JSON.stringify({ version: "1.2.3" }));
  writeFileSync(join(workspaceRoot, "CHANGELOG.md"), changelog);

  const paths = await buildChangelogArtifact(workspaceRoot, "2026-08-27T12:00:00.000Z");

  expect(readFileSync(paths.markdown, "utf8")).toBe(changelog);
  expect(existsSync(paths.pdf)).toBe(true);
  expect(statSync(paths.pdf).size).toBeGreaterThan(0);
});

test("requires the canonical changelog to begin with the package version", () => {
  const releases = parseChangelog(changelog);

  expect(() => assertCurrentRelease("1.2.4", releases)).toThrow(
    "CHANGELOG.md must begin with 1.2.4.",
  );
});
