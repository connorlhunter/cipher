import { describe, expect, test } from "bun:test";
import { basename, resolve } from "node:path";

import {
  runCodeqlScan,
  sarifResultCount,
  type CommandOptions,
  type CommandResult,
  type CommandRunner,
  type ScanFileSystem,
} from "../scripts/security/codeql-scan";
import { requiredToolchains } from "../scripts/toolchains";

const root = resolve("cipher");

function commandResult(stdout = ""): CommandResult {
  return { exitCode: 0, stderr: "", stdout };
}

function createRunner(options?: { version?: string; versionExitCode?: number }): {
  calls: Array<{ command: string[]; options: CommandOptions }>;
  runner: CommandRunner;
} {
  const calls: Array<{ command: string[]; options: CommandOptions }> = [];
  return {
    calls,
    runner: {
      run(command, commandOptions) {
        calls.push({ command, options: commandOptions });
        if (command[1] === "version") {
          return {
            exitCode: options?.versionExitCode ?? 0,
            stderr: "",
            stdout: `${options?.version ?? requiredToolchains.codeql}\n`,
          };
        }
        return commandResult();
      },
    },
  };
}

function createFileSystem(findings: Partial<Record<string, number>> = {}): {
  directories: string[];
  fileSystem: ScanFileSystem;
} {
  const directories: string[] = [];
  return {
    directories,
    fileSystem: {
      makeDirectory(path) {
        directories.push(path);
      },
      readJson(path) {
        const language = basename(path, ".sarif");
        const count = findings[language] ?? 0;
        return { runs: [{ results: Array.from({ length: count }, () => ({})) }] };
      },
    },
  };
}

describe("local CodeQL scan", () => {
  test("pins the required CodeQL CLI exactly", () => {
    expect(requiredToolchains.codeql).toBe("2.26.3");
  });

  test("defers to hosted CodeQL on GitHub Actions without invoking the CLI", () => {
    const messages: string[] = [];
    const runner: CommandRunner = {
      run() {
        throw new Error("CLI should not run");
      },
    };
    const fileSystem: ScanFileSystem = {
      makeDirectory() {
        throw new Error("output should not be created");
      },
      readJson() {
        throw new Error("output should not be read");
      },
    };

    expect(
      runCodeqlScan(
        { githubActions: "true", repositoryRoot: root },
        runner,
        fileSystem,
        (message) => messages.push(message),
      ),
    ).toEqual({ findings: 0, skipped: true });
    expect(messages).toEqual([
      "GITHUB_ACTIONS=true: local CodeQL scan deferred to GitHub's hosted CodeQL analysis.",
    ]);
  });

  test("gives clear setup errors for a missing or mismatched CLI", () => {
    const { fileSystem } = createFileSystem();
    const missing = createRunner({ versionExitCode: 127 });
    expect(() =>
      runCodeqlScan({ repositoryRoot: root }, missing.runner, fileSystem, () => undefined),
    ).toThrow("Install it and put the literal codeql executable on PATH");

    const mismatched = createRunner({ version: "2.26.2" });
    expect(() =>
      runCodeqlScan({ repositoryRoot: root }, mismatched.runner, fileSystem, () => undefined),
    ).toThrow("CodeQL CLI 2.26.3 is required; found 2.26.2");
  });

  test("uses literal CodeQL commands, repository output, bundled suites, and local threats", () => {
    const { calls, runner } = createRunner();
    const { directories, fileSystem } = createFileSystem();

    expect(runCodeqlScan({ repositoryRoot: root }, runner, fileSystem, () => undefined)).toEqual({
      findings: 0,
      skipped: false,
    });

    expect(directories).toEqual([
      resolve(root, ".codeql", "databases"),
      resolve(root, ".codeql", "results"),
      resolve(root, ".codeql", "cache"),
    ]);
    expect(calls[0]?.command).toEqual(["codeql", "version", "--format=terse"]);
    expect(calls.every(({ command }) => command[0] === "codeql")).toBe(true);
    expect(calls.every(({ options }) => options.cwd === root)).toBe(true);

    const createCommands = calls.filter(({ command }) => command[2] === "create");
    expect(createCommands.map(({ command }) => command[4])).toEqual([
      "--language=javascript-typescript",
      "--language=rust",
      "--language=actions",
    ]);
    expect(
      createCommands.every(({ command }) => command.includes("--common-caches=.codeql/cache")),
    ).toBe(true);
    expect(
      createCommands.every(({ command }) =>
        command.includes("--codescanning-config=scripts/security/codeql-config.yml"),
      ),
    ).toBe(true);

    const analyzeCommands = calls.filter(({ command }) => command[2] === "analyze");
    expect(analyzeCommands.map(({ command }) => command[4])).toEqual([
      "codeql/javascript-queries:codeql-suites/javascript-security-extended.qls",
      "codeql/rust-queries:codeql-suites/rust-security-extended.qls",
      "codeql/actions-queries:codeql-suites/actions-security-extended.qls",
    ]);
    expect(
      analyzeCommands.every(
        ({ command }) =>
          command.includes("--threat-model=local") &&
          command.some((argument) => argument.startsWith("--output=.codeql/results/")),
      ),
    ).toBe(true);
  });

  test("fails on every SARIF result without applying a baseline", () => {
    const { calls, runner } = createRunner();
    const { fileSystem } = createFileSystem({ rust: 2 });

    expect(() =>
      runCodeqlScan({ repositoryRoot: root }, runner, fileSystem, () => undefined),
    ).toThrow("CodeQL found 2 SARIF result(s). Review .codeql/results; no baseline is applied.");
    expect(calls).toHaveLength(7);
  });
});

describe("SARIF result counting", () => {
  test("counts results across runs and accepts an omitted results collection", () => {
    expect(
      sarifResultCount({
        runs: [{ results: [{}, {}] }, {}, { results: [{}] }],
      }),
    ).toBe(3);
  });

  test("rejects malformed result collections", () => {
    expect(() => sarifResultCount({ runs: [{ results: {} }] })).toThrow("results are not an array");
  });
});
