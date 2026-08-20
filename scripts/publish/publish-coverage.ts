import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { coveragePaths } from "../coverage/coverage-paths";
import { prepareCoveragePublication } from "../coverage/prepare-coverage-publication";

const projectSlug = "cipher";

export interface CoveragePublishDestination {
  readonly label: string;
  readonly source: string;
  readonly target: string;
}

export interface CoverageInvalidation {
  readonly distributionId: string;
  readonly path: string;
}

export interface PublishCoverageOptions {
  readonly commandRunner?: CommandRunner;
  readonly env?: NodeJS.ProcessEnv;
  readonly workspaceRoot?: string;
}

export interface PublishCoveragePublicationOptions extends PublishCoverageOptions {
  readonly updatedAt?: string;
}

export type CommandRunner = (
  command: string,
  args: ReadonlyArray<string>,
  subject: string,
) => Promise<void>;

/** Normalizes an optional environment value. */
function envValue(value: string | undefined): string {
  return value?.trim() ?? "";
}

/** Joins S3 or CloudFront path parts without duplicate separators. */
function keyPath(...parts: ReadonlyArray<string>): string {
  return parts
    .map((part) => part.trim().replace(/^\/+|\/+$/gu, ""))
    .filter(Boolean)
    .join("/");
}

/** Returns an S3 URI with one trailing slash. */
function s3Uri(bucket: string, key: string): string {
  return key ? `s3://${bucket}/${key}/` : `s3://${bucket}/`;
}

/**
 * Builds the optional durable-source and live destinations for Cipher coverage.
 *
 * @param env - Publication environment.
 * @param workspaceRoot - Cipher checkout containing coverage output.
 * @returns Configured S3 destinations.
 */
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
      "Missing SOURCE_ARTIFACTS_BUCKET or ARTIFACTS_BUCKET for Cipher coverage publication.",
    );
  }

  return destinations;
}

/**
 * Builds the project-scoped CloudFront invalidation.
 *
 * @param env - Publication environment.
 * @returns Zero or one invalidation.
 */
export function coverageInvalidations(
  env: NodeJS.ProcessEnv = process.env,
): CoverageInvalidation[] {
  const distributionId = envValue(env.ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID);

  return distributionId
    ? [
        {
          distributionId,
          path: `/${keyPath(envValue(env.ARTIFACTS_PREFIX), "projects", projectSlug, "coverage", "*")}`,
        },
      ]
    : [];
}

/** Runs one publication command and includes its output in failures. */
export const defaultCommandRunner: CommandRunner = (command, args, subject) =>
  new Promise((resolve, reject) => {
    const child = spawn(command, [...args], { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";

    child.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.on("error", (error: Error) => {
      reject(new Error(`${subject} failed: ${error.message}`));
    });
    child.on("close", (code: number | null) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(
        new Error(
          [`${subject} failed with exit code ${code ?? "unknown"}.`, stdout.trim(), stderr.trim()]
            .filter(Boolean)
            .join("\n"),
        ),
      );
    });
  });

/**
 * Uploads already prepared coverage files to Cipher's project prefix.
 *
 * @param options - Publication collaborators and environment.
 */
export async function publishCoverage(options: PublishCoverageOptions = {}): Promise<void> {
  const env = options.env ?? process.env;
  const commandRunner = options.commandRunner ?? defaultCommandRunner;
  const paths = coveragePaths(options.workspaceRoot);
  const requiredFiles = [
    paths.overview.html,
    paths.overview.pdf,
    paths.typescript.html,
    paths.typescript.pdf,
  ];

  if (requiredFiles.some((path) => !existsSync(path))) {
    throw new Error("Missing Cipher coverage HTML or PDF output. Prepare coverage first.");
  }

  for (const destination of coveragePublishDestinations(env, options.workspaceRoot)) {
    console.log(`Publishing ${destination.label}: ${destination.target}`);
    await commandRunner(
      "aws",
      ["s3", "sync", destination.source, destination.target, "--delete"],
      destination.label,
    );
  }

  for (const invalidation of coverageInvalidations(env)) {
    await commandRunner(
      "aws",
      [
        "cloudfront",
        "create-invalidation",
        "--distribution-id",
        invalidation.distributionId,
        "--paths",
        invalidation.path,
      ],
      "Coverage CloudFront invalidation",
    );
  }

  console.log("Published Cipher coverage artifacts.");
}

/**
 * Creates one project-owned timestamp, renders every page/PDF, and publishes them.
 *
 * @param options - Publication options and optional deterministic timestamp.
 */
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
