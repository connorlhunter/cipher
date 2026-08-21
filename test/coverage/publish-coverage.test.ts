import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, spyOn, test } from "bun:test";
import {
  coverageInvalidations,
  coveragePublishDestinations,
  publishCoverage,
  type CommandRunner,
} from "../../scripts/publish/publish-coverage";

describe("publish coverage", () => {
  let directory = "";

  afterEach(() => {
    if (directory) rmSync(directory, { force: true, recursive: true });
    directory = "";
  });

  test("builds Cipher-scoped source and live destinations", () => {
    directory = mkdtempSync(join(tmpdir(), "cipher-coverage-publish-"));

    expect(
      coveragePublishDestinations(
        {
          ARTIFACTS_BUCKET: "published-artifacts",
          ARTIFACTS_PREFIX: "/site/",
          SOURCE_ARTIFACTS_BUCKET: "source-artifacts",
          SOURCE_ARTIFACTS_PREFIX: "raw",
        },
        directory,
      ),
    ).toEqual([
      {
        label: "Source coverage copy",
        source: join(directory, "coverage"),
        target: "s3://source-artifacts/raw/projects/cipher/coverage/",
      },
      {
        label: "Live coverage artifact",
        source: join(directory, "coverage"),
        target: "s3://published-artifacts/site/projects/cipher/coverage/",
      },
    ]);
  });

  test("builds a Cipher-scoped CloudFront invalidation", () => {
    expect(
      coverageInvalidations({
        ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID: "DISTRIBUTION",
        ARTIFACTS_PREFIX: "site",
      }),
    ).toEqual([
      {
        distributionId: "DISTRIBUTION",
        path: "/site/projects/cipher/coverage/*",
      },
    ]);
  });

  test("publishes complete HTML and PDF output", async () => {
    const commands: Array<{ args: ReadonlyArray<string>; subject: string }> = [];
    const commandRunner: CommandRunner = async (_command, args, subject) => {
      commands.push({ args, subject });
    };
    spyOn(console, "log").mockImplementation(() => undefined);
    directory = mkdtempSync(join(tmpdir(), "cipher-coverage-publish-"));
    const coverage = join(directory, "coverage");
    const rust = join(coverage, "rust");
    const typescript = join(coverage, "typescript");
    mkdirSync(rust, { recursive: true });
    mkdirSync(typescript, { recursive: true });
    for (const path of [
      join(coverage, "index.html"),
      join(coverage, "index.pdf"),
      join(rust, "index.html"),
      join(rust, "index.pdf"),
      join(typescript, "index.html"),
      join(typescript, "index.pdf"),
    ]) {
      writeFileSync(path, path.endsWith(".pdf") ? "%PDF-1.4" : "<html>coverage</html>");
    }

    await publishCoverage({
      commandRunner,
      env: {
        ARTIFACTS_BUCKET: "published-artifacts",
        ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID: "DISTRIBUTION",
      },
      workspaceRoot: directory,
    });

    expect(commands).toEqual([
      {
        args: [
          "s3",
          "sync",
          coverage,
          "s3://published-artifacts/projects/cipher/coverage/",
          "--delete",
        ],
        subject: "Live coverage artifact",
      },
      {
        args: [
          "cloudfront",
          "create-invalidation",
          "--distribution-id",
          "DISTRIBUTION",
          "--paths",
          "/projects/cipher/coverage/*",
        ],
        subject: "Coverage CloudFront invalidation",
      },
    ]);
  });

  test("requires destinations and complete output", async () => {
    expect(() => coveragePublishDestinations({})).toThrow("Missing SOURCE_ARTIFACTS_BUCKET");
    directory = mkdtempSync(join(tmpdir(), "cipher-coverage-publish-"));

    await expect(
      publishCoverage({
        commandRunner: async () => undefined,
        env: { ARTIFACTS_BUCKET: "published-artifacts" },
        workspaceRoot: directory,
      }),
    ).rejects.toThrow("Missing Cipher coverage HTML or PDF output");
  });
});
