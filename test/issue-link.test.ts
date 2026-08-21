import { describe, expect, test } from "bun:test";

import {
  assertLinkedIssue,
  hasLinkedIssue,
  isDependabotPullRequest,
  runIssueLinkCheck,
} from "../scripts/issue-link";

describe("pull request issue links", () => {
  test.each([
    "Closes #200",
    "Fixes #48\n\nAdds the shared protocol primitives.",
    "Resolves connorlhunter/cipher#49",
    "Related to #50",
    "Tracks #51",
    "Implements #6",
    "References #199",
    "Refer to #200 for the rollout plan.",
    "Closes https://github.com/connorlhunter/cipher/issues/200",
  ])("accepts %s", (pullRequestBody) => {
    expect(hasLinkedIssue(pullRequestBody)).toBe(true);
    expect(() => assertLinkedIssue(pullRequestBody)).not.toThrow();
  });

  test.each([
    "",
    "## What changed\n\nAdds a missing check.",
    "Issue 200 tracks this work.",
    "Closes #<issue-number>",
    "Closes another-owner/another-repository#200",
    "https://github.com/connorlhunter/cipher/pull/200",
  ])("rejects %s", (pullRequestBody) => {
    expect(hasLinkedIssue(pullRequestBody)).toBe(false);
    expect(() => assertLinkedIssue(pullRequestBody)).toThrow("must link a Cipher issue");
  });
});

describe("pull request issue link command", () => {
  test("validates the pull request body", async () => {
    await expect(
      runIssueLinkCheck(["--pull-request-body"], { CIPHER_PULL_REQUEST_BODY: "Closes #200" }),
    ).resolves.toBeUndefined();
  });

  test("exempts a Dependabot pull request", async () => {
    expect(isDependabotPullRequest("dependabot[bot]")).toBe(true);
    expect(isDependabotPullRequest("connorlhunter")).toBe(false);
    await expect(
      runIssueLinkCheck(["--pull-request-body"], {
        CIPHER_PULL_REQUEST_AUTHOR: "dependabot[bot]",
      }),
    ).resolves.toBeUndefined();
  });

  test("rejects a missing issue link and unsupported invocations", async () => {
    await expect(runIssueLinkCheck(["--pull-request-body"], {})).rejects.toThrow(
      "must link a Cipher issue",
    );
    await expect(runIssueLinkCheck(["--unexpected"], {})).rejects.toThrow("Usage:");
  });
});
