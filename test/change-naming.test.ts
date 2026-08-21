import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

import {
  assertAllowedBranchName,
  assertAllowedChangeTitle,
  currentBranchName,
  isAllowedBranchName,
  isAllowedChangeTitle,
  runChangeNaming,
} from "../scripts/change-naming";

describe("branch naming", () => {
  test.each([
    "main",
    "feat/semantic-change-naming",
    "fix/startup-error",
    "chore/update-dependencies",
    "docs/release-guide",
    "test/realtime-reconnect",
    "refactor/config-loader",
    "release/1.4.6",
    "release/0.1.0-prealpha.1",
    "release/2.0.0-rc.1+build.4",
    "dependabot/npm_and_yarn/typescript-6.0.3",
  ])("accepts %s", (branchName) => {
    expect(isAllowedBranchName(branchName)).toBe(true);
  });

  test.each([
    "feature/semantic-naming",
    "feat/Semantic-Naming",
    "feat/semantic_naming",
    "feat/semantic/naming",
    "release/v1.4.6",
    "release/01.4.6",
    "dependabot/",
    "",
  ])("rejects %s", (branchName) => {
    expect(isAllowedBranchName(branchName)).toBe(false);
    expect(() => assertAllowedBranchName(branchName)).toThrow("Invalid branch name");
  });

  test("prefers the pull request head branch and falls back to Git", () => {
    expect(
      currentBranchName(
        { GITHUB_HEAD_REF: "feat/from-pull-request", GITHUB_REF_NAME: "169/merge" },
        () => "feat/from-git",
      ),
    ).toBe("feat/from-pull-request");
    expect(currentBranchName({}, () => "feat/from-git\n")).toBe("feat/from-git");
  });
});

describe("change title naming", () => {
  test.each([
    "feat: add message search",
    "fix(config): reject an invalid account",
    "chore(release): prepare 0.1.0-prealpha.1",
    "docs!: replace the deployment guide",
    "test(realtime)!: cover reconnect failure",
    "refactor: simplify route ownership",
  ])("accepts %s", (title) => {
    expect(isAllowedChangeTitle(title)).toBe(true);
  });

  test.each([
    "Feature: add message search",
    "feat add message search",
    "feat(scope_with_space): add search",
    "feat:",
    "ci: update workflow",
    "",
  ])("rejects %s", (title) => {
    expect(isAllowedChangeTitle(title)).toBe(false);
    expect(() => assertAllowedChangeTitle(title, "title")).toThrow("Invalid title");
  });
});

describe("change naming command", () => {
  test("validates branch and pull-request modes", async () => {
    await expect(
      runChangeNaming(["--branch"], { GITHUB_HEAD_REF: "feat/coverage-quality" }),
    ).resolves.toBeUndefined();
    await expect(
      runChangeNaming(["--pull-request-title"], {
        CIPHER_PULL_REQUEST_TITLE: "test: enforce coverage quality",
      }),
    ).resolves.toBeUndefined();
  });

  test("rejects unsupported command invocations", async () => {
    await expect(runChangeNaming(["--branch", "unexpected"], {})).rejects.toThrow("Usage:");
  });

  test("reads and validates a commit subject file", async () => {
    const directory = mkdtempSync(join(tmpdir(), "cipher-change-naming-"));
    try {
      const message = join(directory, "message");
      writeFileSync(message, "fix(coverage): enforce 95 percent\n\nbody");
      await expect(
        runChangeNaming(["--commit-message-file", message], {}),
      ).resolves.toBeUndefined();
      expect(currentBranchName()).toMatch(/^(?:main|[a-z]+\/)/u);
    } finally {
      rmSync(directory, { force: true, recursive: true });
    }
  });
});
