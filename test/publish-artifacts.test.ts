import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "bun:test";
import { changelogPaths } from "../scripts/changelog/changelog-artifact";
import { coveragePaths } from "../scripts/coverage/coverage-paths";
import {
  publishChangelog,
  publishChangelogPublication,
  type PublishChangelogOptions,
} from "../scripts/publish/publish-changelog";
import {
  publishCoverage,
  publishCoveragePublication,
  type CommandRunner,
  type PublishCoverageOptions,
} from "../scripts/publish/publish-coverage";

let workspaceRoot = "";

afterEach(() => {
  if (workspaceRoot) rmSync(workspaceRoot, { force: true, recursive: true });
  workspaceRoot = "";
});

function commandRecorder(calls: Array<ReadonlyArray<string>>): CommandRunner {
  return async (command, args, subject) => {
    calls.push([command, ...args, subject]);
  };
}

function artifactEnvironment(): NodeJS.ProcessEnv {
  return {
    ARTIFACTS_BUCKET: "live-artifacts",
    ARTIFACTS_PREFIX: "public",
    ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID: "distribution-id",
    SOURCE_ARTIFACTS_BUCKET: "source-artifacts",
    SOURCE_ARTIFACTS_PREFIX: "archive",
  };
}

test("publishes the coverage JSON and PDF to both project namespaces", async () => {
  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-coverage-publish-"));
  const paths = coveragePaths(workspaceRoot);
  mkdirSync(paths.directory, { recursive: true });
  writeFileSync(paths.json, "{}");
  writeFileSync(paths.pdf, "pdf");
  const calls: Array<ReadonlyArray<string>> = [];
  const options: PublishCoverageOptions = {
    commandRunner: commandRecorder(calls),
    env: artifactEnvironment(),
    workspaceRoot,
  };

  await publishCoverage(options);

  expect(calls).toEqual([
    [
      "aws",
      "s3",
      "sync",
      paths.directory,
      "s3://source-artifacts/archive/projects/cipher/coverage/",
      "--exclude",
      "lcov.info",
      "--exclude",
      "rust.lcov",
      "--delete",
      "Source coverage copy",
    ],
    [
      "aws",
      "s3",
      "sync",
      paths.directory,
      "s3://live-artifacts/public/projects/cipher/coverage/",
      "--exclude",
      "lcov.info",
      "--exclude",
      "rust.lcov",
      "--delete",
      "Live coverage artifact",
    ],
    [
      "aws",
      "cloudfront",
      "create-invalidation",
      "--distribution-id",
      "distribution-id",
      "--paths",
      "/public/projects/cipher/coverage/*",
      "Coverage CloudFront invalidation",
    ],
  ]);
});

test("publishes changelog Markdown and PDF to both project namespaces", async () => {
  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-changelog-publish-"));
  const paths = changelogPaths(workspaceRoot);
  mkdirSync(paths.directory, { recursive: true });
  writeFileSync(paths.markdown, "# Changelog\n");
  writeFileSync(paths.pdf, "pdf");
  const calls: Array<ReadonlyArray<string>> = [];
  const options: PublishChangelogOptions = {
    commandRunner: commandRecorder(calls),
    env: artifactEnvironment(),
    workspaceRoot,
  };

  await publishChangelog(options);

  expect(calls).toEqual([
    [
      "aws",
      "s3",
      "sync",
      paths.directory,
      "s3://source-artifacts/archive/projects/cipher/changelog/",
      "--delete",
      "Source changelog copy",
    ],
    [
      "aws",
      "s3",
      "sync",
      paths.directory,
      "s3://live-artifacts/public/projects/cipher/changelog/",
      "--delete",
      "Live changelog artifact",
    ],
    [
      "aws",
      "cloudfront",
      "create-invalidation",
      "--distribution-id",
      "distribution-id",
      "--paths",
      "/public/projects/cipher/changelog/*",
      "Changelog CloudFront invalidation",
    ],
  ]);
});

test("builds each artifact before publishing it", async () => {
  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-artifact-publication-"));
  const coverage = coveragePaths(workspaceRoot);
  mkdirSync(coverage.directory, { recursive: true });
  const lcov = "SF:src/example.ts\nFNF:1\nFNH:1\nLF:1\nLH:1\nend_of_record\n";
  writeFileSync(coverage.typescriptLcov, lcov);
  writeFileSync(coverage.rustLcov, lcov);
  mkdirSync(join(coverage.directory, "typescript"), { recursive: true });
  writeFileSync(join(coverage.directory, "index.html"), "legacy coverage");
  writeFileSync(join(coverage.directory, "typescript", "index.html"), "legacy coverage");
  writeFileSync(join(workspaceRoot, "package.json"), JSON.stringify({ version: "1.2.3" }));
  writeFileSync(
    join(workspaceRoot, "CHANGELOG.md"),
    "# Changelog\n\n## [1.2.3] - 2026-08-27\n\n### Added\n\n- Publish artifacts.\n",
  );
  const calls: Array<ReadonlyArray<string>> = [];
  const options = {
    commandRunner: commandRecorder(calls),
    env: artifactEnvironment(),
    workspaceRoot,
  };

  await publishCoveragePublication({ ...options, updatedAt: "2026-08-27T12:00:00.000Z" });
  await publishChangelogPublication(options);

  expect(calls).toHaveLength(6);
  expect(JSON.parse(await Bun.file(coverage.json).text())).toMatchObject({ schemaVersion: 2 });
  expect(await Bun.file(coverage.pdf).exists()).toBe(true);
  expect(await Bun.file(changelogPaths(workspaceRoot).pdf).exists()).toBe(true);
  expect(await Bun.file(join(coverage.directory, "index.html")).exists()).toBe(false);
  expect(await Bun.file(join(coverage.directory, "typescript", "index.html")).exists()).toBe(false);
});

test("requires a configured artifact bucket before publishing", async () => {
  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-artifact-publish-"));
  const paths = changelogPaths(workspaceRoot);
  mkdirSync(paths.directory, { recursive: true });
  writeFileSync(paths.markdown, "# Changelog\n");
  writeFileSync(paths.pdf, "pdf");

  await expect(publishChangelog({ env: {}, workspaceRoot })).rejects.toThrow(
    "Missing SOURCE_ARTIFACTS_BUCKET or ARTIFACTS_BUCKET for changelog publishing.",
  );
});

test("requires prepared artifact files before publishing", async () => {
  workspaceRoot = mkdtempSync(join(tmpdir(), "cipher-missing-artifact-publish-"));

  await expect(publishCoverage({ env: artifactEnvironment(), workspaceRoot })).rejects.toThrow(
    "Missing coverage artifacts:",
  );
  await expect(publishChangelog({ env: artifactEnvironment(), workspaceRoot })).rejects.toThrow(
    "Missing changelog artifacts.",
  );
});
