import { describe, expect, test } from "bun:test";

import {
  assertAllowedBranchName,
  assertAllowedChangeTitle,
  currentBranchName,
  isAllowedBranchName,
  isAllowedChangeTitle,
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
