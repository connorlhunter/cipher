import { existsSync } from "node:fs";
import { buildChangelogArtifact, changelogPaths } from "../changelog/changelog-artifact";
import { defaultCommandRunner, type CommandRunner } from "./command-runner";

const projectSlug = "cipher";

export interface PublishChangelogOptions {
  readonly commandRunner?: CommandRunner;
  readonly env?: NodeJS.ProcessEnv;
  readonly invalidate?: boolean;
  readonly workspaceRoot?: string;
}

function envValue(value: string | undefined): string {
  return value?.trim() ?? "";
}

function keyPath(...parts: ReadonlyArray<string>): string {
  return parts
    .map((part) => part.trim().replace(/^\/+|\/+$/gu, ""))
    .filter(Boolean)
    .join("/");
}

function s3Uri(bucket: string, key: string): string {
  return key ? `s3://${bucket}/${key}/` : `s3://${bucket}/`;
}

/** Publishes changelog Markdown/PDF to the same project namespace as coverage. */
export async function publishChangelog(options: PublishChangelogOptions = {}): Promise<void> {
  const env = options.env ?? process.env;
  const root = options.workspaceRoot ?? process.cwd();
  const paths = changelogPaths(root);
  if (!existsSync(paths.markdown) || !existsSync(paths.pdf))
    throw new Error("Missing changelog artifacts.");
  const runner = options.commandRunner ?? defaultCommandRunner;
  const destinations = [
    [
      envValue(env.SOURCE_ARTIFACTS_BUCKET),
      envValue(env.SOURCE_ARTIFACTS_PREFIX),
      "Source changelog copy",
    ],
    [envValue(env.ARTIFACTS_BUCKET), envValue(env.ARTIFACTS_PREFIX), "Live changelog artifact"],
  ] as const;
  const configured = destinations.filter(([bucket]) => Boolean(bucket));
  if (configured.length === 0)
    throw new Error(
      "Missing SOURCE_ARTIFACTS_BUCKET or ARTIFACTS_BUCKET for changelog publishing.",
    );
  for (const [bucket, prefix, label] of configured) {
    await runner(
      "aws",
      [
        "s3",
        "sync",
        paths.directory,
        s3Uri(bucket, keyPath(prefix, "projects", projectSlug, "changelog")),
        "--delete",
      ],
      label,
    );
  }
  const distributionId = envValue(env.ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID);
  if ((options.invalidate ?? true) && distributionId) {
    await runner(
      "aws",
      [
        "cloudfront",
        "create-invalidation",
        "--distribution-id",
        distributionId,
        "--paths",
        `/${keyPath(envValue(env.ARTIFACTS_PREFIX), "projects", projectSlug, "changelog", "*")}`,
      ],
      "Changelog CloudFront invalidation",
    );
  }
}

/** Builds the canonical changelog artifact immediately before upload. */
export async function publishChangelogPublication(
  options: PublishChangelogOptions = {},
): Promise<void> {
  await buildChangelogArtifact(options.workspaceRoot);
  await publishChangelog(options);
}

if (import.meta.main) {
  try {
    await publishChangelogPublication();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
