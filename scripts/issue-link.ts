const issueReferencePattern = new RegExp(
  String.raw`\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?|related\s+to|track(?:s|ed)?|implement(?:s|ed)?|reference(?:s|d)?|refer(?:s|red)?\s+to)\s+(?:(?:connorlhunter/cipher)?#\d+|https://github\.com/connorlhunter/cipher/issues/\d+)\b`,
  "iu",
);
const dependabotLogin = "dependabot[bot]";

type Environment = Readonly<Record<string, string | undefined>>;

/** Returns whether a pull request description contains a recognized Cipher issue link. */
export function hasLinkedIssue(pullRequestBody: string): boolean {
  return issueReferencePattern.test(pullRequestBody);
}

/** Returns whether the pull request was opened by Dependabot. */
export function isDependabotPullRequest(pullRequestAuthor: string | undefined): boolean {
  return pullRequestAuthor === dependabotLogin;
}

/** Throws when a pull request description does not link a Cipher issue. */
export function assertLinkedIssue(pullRequestBody: string): void {
  if (!hasLinkedIssue(pullRequestBody)) {
    throw new Error(
      'Pull request descriptions must link a Cipher issue. Use a phrase such as "Closes #123" or "Related to #123".',
    );
  }
}

/** Validates the pull request description supplied by GitHub Actions. */
export async function runIssueLinkCheck(
  arguments_: string[],
  environment: Environment,
): Promise<void> {
  if (arguments_.length === 1 && arguments_[0] === "--pull-request-body") {
    if (isDependabotPullRequest(environment.CIPHER_PULL_REQUEST_AUTHOR)) {
      return;
    }

    assertLinkedIssue(environment.CIPHER_PULL_REQUEST_BODY ?? "");
    return;
  }

  throw new Error("Usage: issue-link.ts --pull-request-body");
}

if (import.meta.main) {
  await runIssueLinkCheck(process.argv.slice(2), process.env);
}
