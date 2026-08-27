import { existsSync } from "node:fs";
import { coveragePaths } from "../coverage/coverage-paths";
import { prepareCoveragePublication } from "../coverage/prepare-coverage-publication";
import { defaultCommandRunner, type CommandRunner } from "./command-runner";

const projectSlug = "cipher";

export { defaultCommandRunner, type CommandRunner } from "./command-runner";

export interface CoveragePublishDestination {
  readonly label: string;
  readonly source: string;
  readonly target: string;
}

export interface PublishCoverageOptions {
  readonly commandRunner?: CommandRunner;
  readonly env?: NodeJS.ProcessEnv;
  readonly invalidate?: boolean;
  readonly workspaceRoot?: string;
}

export interface PublishCoveragePublicationOptions extends PublishCoverageOptions {
  readonly updatedAt?: string;
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

/** Resolves source and live coverage destinations from the publishing environment. */
export function coveragePublishDestinations(
  env: NodeJS.ProcessEnv = process.env,
  workspaceRoot = process.cwd(),
): CoveragePublishDestination[] {
  const source = coveragePaths(workspaceRoot).directory;
  const destinations: CoveragePublishDestination[] = [];
  const sourceBucket = envValue(env.SOURCE_ARTIFACTS_BUCKET);
  const artifactsBucket = envValue(env.ARTIFACTS_BUCKET);
  if (sourceBucket) {
    destinations.push({
      label: "Source coverage copy",
      source,
      target: s3Uri(
        sourceBucket,
        keyPath(envValue(env.SOURCE_ARTIFACTS_PREFIX), "projects", projectSlug, "coverage"),
      ),
    });
  }
  if (artifactsBucket) {
    destinations.push({
      label: "Live coverage artifact",
      source,
      target: s3Uri(
        artifactsBucket,
        keyPath(envValue(env.ARTIFACTS_PREFIX), "projects", projectSlug, "coverage"),
      ),
    });
  }
  if (destinations.length === 0) {
    throw new Error(
      "Missing SOURCE_ARTIFACTS_BUCKET or ARTIFACTS_BUCKET for Cipher coverage publishing.",
    );
  }
  return destinations;
}

/** Uploads JSON/PDF coverage output and clears retired files from the artifact prefix. */
export async function publishCoverage(options: PublishCoverageOptions = {}): Promise<void> {
  const paths = coveragePaths(options.workspaceRoot);
  if (!existsSync(paths.json) || !existsSync(paths.pdf)) {
    throw new Error(`Missing coverage artifacts: ${paths.json} or ${paths.pdf}.`);
  }
  const env = options.env ?? process.env;
  const runner = options.commandRunner ?? defaultCommandRunner;
  for (const destination of coveragePublishDestinations(env, options.workspaceRoot)) {
    await runner(
      "aws",
      [
        "s3",
        "sync",
        destination.source,
        destination.target,
        "--exclude",
        "lcov.info",
        "--exclude",
        "rust.lcov",
        "--delete",
      ],
      destination.label,
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
        `/${keyPath(envValue(env.ARTIFACTS_PREFIX), "projects", projectSlug, "coverage", "*")}`,
      ],
      "Coverage CloudFront invalidation",
    );
  }
}

/** Builds the project-owned artifact pair, then publishes it. */
export async function publishCoveragePublication(
  options: PublishCoveragePublicationOptions = {},
): Promise<void> {
  await prepareCoveragePublication(options.workspaceRoot, options.updatedAt);
  await publishCoverage(options);
}

if (import.meta.main) {
  try {
    await publishCoveragePublication();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
