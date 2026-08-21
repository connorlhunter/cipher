import { mkdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { requiredToolchains } from "../toolchains";

const repositoryRoot = fileURLToPath(new URL("../../", import.meta.url));
const outputRoot = ".codeql";
const cacheDirectory = `${outputRoot}/cache`;

/** One language and bundled query suite included in the local security scan. */
interface ScanTarget {
  language: "actions" | "javascript-typescript" | "rust";
  suite: string;
}

const scanTargets: ReadonlyArray<ScanTarget> = [
  {
    language: "javascript-typescript",
    suite: "codeql/javascript-queries:codeql-suites/javascript-security-extended.qls",
  },
  {
    language: "rust",
    suite: "codeql/rust-queries:codeql-suites/rust-security-extended.qls",
  },
  {
    language: "actions",
    suite: "codeql/actions-queries:codeql-suites/actions-security-extended.qls",
  },
];

/** Captured outcome from one executable invocation. */
export interface CommandResult {
  exitCode: number;
  stderr: string;
  stdout: string;
}

/** Options controlling one executable invocation. */
export interface CommandOptions {
  captureOutput: boolean;
  cwd: string;
}

/** Executes exact argument arrays without a shell. */
export interface CommandRunner {
  run(command: string[], options: CommandOptions): CommandResult;
}

/** Repository-local filesystem operations needed by the scanner. */
export interface ScanFileSystem {
  makeDirectory(path: string): void;
  readJson(path: string): unknown;
}

/** Environment values that alter where CodeQL runs. */
export interface ScanEnvironment {
  githubActions?: string;
  repositoryRoot: string;
}

/** Summary returned after a local scan or hosted-CI deferral. */
export interface ScanSummary {
  findings: number;
  skipped: boolean;
}

export const liveRunner: CommandRunner = {
  run(command, options) {
    if (options.captureOutput) {
      const result = Bun.spawnSync(command, {
        cwd: options.cwd,
        stderr: "pipe",
        stdout: "pipe",
      });
      return {
        exitCode: result.exitCode,
        stderr: new TextDecoder().decode(result.stderr),
        stdout: new TextDecoder().decode(result.stdout),
      };
    }

    const result = Bun.spawnSync(command, {
      cwd: options.cwd,
      stderr: "inherit",
      stdout: "inherit",
    });
    return { exitCode: result.exitCode, stderr: "", stdout: "" };
  },
};

export const liveFileSystem: ScanFileSystem = {
  makeDirectory(path) {
    mkdirSync(path, { recursive: true });
  },
  readJson(path) {
    try {
      return JSON.parse(readFileSync(path, "utf8")) as unknown;
    } catch {
      throw new Error(`Could not read CodeQL SARIF output at ${path}.`);
    }
  },
};

/**
 * @param target - Language target being extracted.
 * @returns Exact CodeQL database-creation command for the target.
 */
export function databaseCreateCommand(target: ScanTarget): string[] {
  return [
    "codeql",
    "database",
    "create",
    `${outputRoot}/databases/${target.language}`,
    `--language=${target.language}`,
    "--source-root=.",
    "--codescanning-config=scripts/security/codeql-config.yml",
    "--overwrite",
    "--quiet",
    "--threads=0",
    `--common-caches=${cacheDirectory}`,
  ];
}

/**
 * @param target - Language target being analyzed.
 * @returns Exact CodeQL analysis command for the target.
 */
export function databaseAnalyzeCommand(target: ScanTarget): string[] {
  return [
    "codeql",
    "database",
    "analyze",
    `${outputRoot}/databases/${target.language}`,
    target.suite,
    "--format=sarifv2.1.0",
    `--output=${outputRoot}/results/${target.language}.sarif`,
    "--threat-model=local",
    "--quiet",
    "--threads=0",
    `--common-caches=${cacheDirectory}`,
  ];
}

/**
 * @param sarif - Parsed SARIF document.
 * @returns Total alert results across all SARIF runs.
 */
export function sarifResultCount(sarif: unknown): number {
  if (typeof sarif !== "object" || sarif === null || !("runs" in sarif)) {
    throw new Error("CodeQL produced invalid SARIF: runs are missing.");
  }
  const runs = sarif.runs;
  if (!Array.isArray(runs)) {
    throw new Error("CodeQL produced invalid SARIF: runs are not an array.");
  }

  let count = 0;
  for (const run of runs) {
    if (typeof run !== "object" || run === null) {
      throw new Error("CodeQL produced invalid SARIF: a run is not an object.");
    }
    if (!("results" in run) || run.results === undefined) {
      continue;
    }
    if (!Array.isArray(run.results)) {
      throw new Error("CodeQL produced invalid SARIF: results are not an array.");
    }
    count += run.results.length;
  }
  return count;
}

/**
 * Ensures the literal CodeQL executable on PATH exactly matches the repository pin.
 *
 * @param runner - Command runner used to inspect the CLI.
 * @param root - Repository root used as the command working directory.
 * @returns Nothing; throws with setup guidance when CodeQL is missing or mismatched.
 */
function assertCodeqlVersion(runner: CommandRunner, root: string): void {
  let result: CommandResult;
  try {
    result = runner.run(["codeql", "version", "--format=terse"], {
      captureOutput: true,
      cwd: root,
    });
  } catch {
    throw new Error(
      `CodeQL CLI ${requiredToolchains.codeql} is required. Install it and put the literal codeql executable on PATH.`,
    );
  }

  if (result.exitCode !== 0) {
    throw new Error(
      `CodeQL CLI ${requiredToolchains.codeql} is required. Install it and put the literal codeql executable on PATH.`,
    );
  }

  const installedVersion = result.stdout.trim();
  if (installedVersion !== requiredToolchains.codeql) {
    throw new Error(
      `CodeQL CLI ${requiredToolchains.codeql} is required; found ${installedVersion || "an unreadable version"}. Install the pinned version and ensure codeql on PATH resolves to it.`,
    );
  }
}

/**
 * Runs local security-extended CodeQL analysis and rejects every SARIF finding.
 *
 * @param environment - Hosted-CI signal and repository root.
 * @param runner - Command runner used for CodeQL.
 * @param fileSystem - Filesystem used for ignored output and SARIF reads.
 * @param log - User-facing progress logger.
 * @returns Scan disposition and total finding count.
 */
export function runCodeqlScan(
  environment: ScanEnvironment,
  runner: CommandRunner,
  fileSystem: ScanFileSystem,
  log: (message: string) => void,
): ScanSummary {
  if (environment.githubActions === "true") {
    log("GITHUB_ACTIONS=true: local CodeQL scan deferred to GitHub's hosted CodeQL analysis.");
    return { findings: 0, skipped: true };
  }

  assertCodeqlVersion(runner, environment.repositoryRoot);
  for (const directory of ["databases", "results", "cache"]) {
    fileSystem.makeDirectory(resolve(environment.repositoryRoot, outputRoot, directory));
  }

  let findings = 0;
  for (const target of scanTargets) {
    log(`Creating CodeQL ${target.language} database...`);
    const createResult = runner.run(databaseCreateCommand(target), {
      captureOutput: false,
      cwd: environment.repositoryRoot,
    });
    if (createResult.exitCode !== 0) {
      throw new Error(`CodeQL database creation failed for ${target.language}.`);
    }

    log(`Analyzing ${target.language} with its bundled security-extended suite...`);
    const analyzeResult = runner.run(databaseAnalyzeCommand(target), {
      captureOutput: false,
      cwd: environment.repositoryRoot,
    });
    if (analyzeResult.exitCode !== 0) {
      throw new Error(`CodeQL analysis failed for ${target.language}.`);
    }

    const resultPath = resolve(
      environment.repositoryRoot,
      outputRoot,
      "results",
      `${target.language}.sarif`,
    );
    const targetFindings = sarifResultCount(fileSystem.readJson(resultPath));
    findings += targetFindings;
    log(`${target.language}: ${targetFindings} SARIF result(s).`);
  }

  if (findings > 0) {
    throw new Error(
      `CodeQL found ${findings} SARIF result(s). Review ${outputRoot}/results; no baseline is applied.`,
    );
  }

  log("CodeQL scan passed with 0 SARIF results.");
  return { findings, skipped: false };
}

/** Runs the local scanner and reports a non-sensitive CLI failure. */
export function runCodeqlCli(
  environment: ScanEnvironment,
  runner: CommandRunner,
  fileSystem: ScanFileSystem,
  log: (message: string) => void,
  errorLog: (message: string) => void,
): ScanSummary | undefined {
  try {
    return runCodeqlScan(environment, runner, fileSystem, log);
  } catch (error) {
    const message = error instanceof Error ? error.message : "Unknown CodeQL scan failure.";
    errorLog(`CodeQL scan failed: ${message}`);
    return undefined;
  }
}

if (import.meta.main) {
  const summary = runCodeqlCli(
    { githubActions: process.env.GITHUB_ACTIONS, repositoryRoot },
    liveRunner,
    liveFileSystem,
    console.log,
    console.error,
  );
  if (summary === undefined) process.exitCode = 1;
}
