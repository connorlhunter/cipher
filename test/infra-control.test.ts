import { describe, expect, test } from "bun:test";

import { loadInfrastructureConfig } from "../config/environment";
import {
  parseArguments,
  plannedCommands,
  runInfrastructureControl,
  liveRunner,
  logInfrastructureControlResult,
  type CommandRunner,
} from "../scripts/infra-control";

const expectedAccount = "123456789012";
const config = loadInfrastructureConfig({
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_STATE_STACK: "CipherProductionState",
  CIPHER_CONTROL_STACK: "CipherProductionControl",
  CIPHER_NETWORK_STACK: "CipherProductionNetwork",
  CIPHER_RUNTIME_STACK: "CipherProductionRuntime",
});

function confirmation(action: "pause" | "resume" | "destroy-all"): string {
  const verb = action === "destroy-all" ? "UNLOCK" : action.toUpperCase();
  return `${verb}-CIPHER-PRODUCTION-${expectedAccount}-us-east-1`;
}

function createRunner(options?: { account?: string; existingStacks?: string[] }): {
  calls: string[][];
  runner: CommandRunner;
} {
  const account = options?.account ?? expectedAccount;
  const existingStacks = new Set(options?.existingStacks ?? []);
  const calls: string[][] = [];
  return {
    calls,
    runner: {
      run(command) {
        calls.push(command);
        if (command[0] === "aws" && command[1] === "sts") {
          return { exitCode: 0, stderr: "", stdout: `${account}\n` };
        }
        if (command[0] === "aws" && command[1] === "cloudformation") {
          const stack = command[command.indexOf("--stack-name") + 1];
          if (existingStacks.has(stack)) {
            return { exitCode: 0, stderr: "", stdout: "{}" };
          }
          return {
            exitCode: 255,
            stderr: `Stack with id ${stack} does not exist`,
            stdout: "",
          };
        }
        return { exitCode: 0, stderr: "", stdout: "" };
      },
    },
  };
}

const interactiveEnvironment = { accountId: expectedAccount, isInteractive: true };

describe("infrastructure controls", () => {
  test("requires one known action and rejects unknown options", () => {
    expect(() => parseArguments([])).toThrow("Choose one action");
    expect(() => parseArguments(["pause", "--unsafe"])).toThrow(
      "Unknown infrastructure control option",
    );
  });

  test("does not contact AWS during a dry run", () => {
    const { calls, runner } = createRunner();

    const commands = runInfrastructureControl(
      ["pause", `--confirm=${confirmation("pause")}`, "--dry-run"],
      interactiveEnvironment,
      runner,
      config,
    );

    expect(calls).toEqual([]);
    expect(commands).toEqual([
      "npm --prefix infra exec cdk -- destroy CipherProductionRuntime --force",
      "npm --prefix infra exec cdk -- destroy CipherProductionNetwork --force",
    ]);
  });

  test("requires action-specific confirmation phrases", () => {
    const { runner } = createRunner();

    expect(() =>
      runInfrastructureControl(
        ["destroy-all", `--confirm=${confirmation("destroy-all")}`],
        interactiveEnvironment,
        runner,
        config,
      ),
    ).toThrow("--destroy-confirm=DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA");
  });

  test("refuses non-interactive changes", () => {
    const { calls, runner } = createRunner();

    expect(() =>
      runInfrastructureControl(
        ["resume", `--confirm=${confirmation("resume")}`],
        { accountId: expectedAccount, isInteractive: false },
        runner,
        config,
      ),
    ).toThrow("outside an interactive terminal");

    expect(calls).toEqual([]);
  });

  test("stops after an account mismatch without checking stacks", () => {
    const { calls, runner } = createRunner({ account: "000000000000" });

    expect(() =>
      runInfrastructureControl(
        ["resume", `--confirm=${confirmation("resume")}`],
        interactiveEnvironment,
        runner,
        config,
      ),
    ).toThrow("active AWS account is not Cipher production");

    expect(calls).toHaveLength(1);
    expect(calls[0]?.slice(0, 3)).toEqual(["aws", "sts", "get-caller-identity"]);
  });

  test("makes pause idempotent when runtime stacks are already absent", () => {
    const { calls, runner } = createRunner();

    const commands = runInfrastructureControl(
      ["pause", `--confirm=${confirmation("pause")}`],
      interactiveEnvironment,
      runner,
      config,
    );

    expect(commands).toEqual([]);
    expect(calls).toHaveLength(3);
    expect(calls.every((command) => command[0] === "aws")).toBe(true);
  });

  test("uses only exact Cipher stack names for full destruction", () => {
    const { calls, runner } = createRunner({
      existingStacks: [
        "CipherProductionState",
        "CipherProductionControl",
        "CipherProductionNetwork",
        "CipherProductionRuntime",
      ],
    });

    runInfrastructureControl(
      [
        "destroy-all",
        `--confirm=${confirmation("destroy-all")}`,
        `--destroy-confirm=DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA-${expectedAccount}-us-east-1`,
      ],
      interactiveEnvironment,
      runner,
      config,
    );

    const cdkCalls = calls.filter((command) => command[0] === "npm");
    expect(cdkCalls).toEqual([
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "deploy",
        "CipherProductionState",
        "CipherProductionControl",
        "--context",
        "cipher:allow-persistent-destruction=true",
        "--require-approval",
        "any-change",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "destroy",
        "CipherProductionRuntime",
        "--force",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "destroy",
        "CipherProductionNetwork",
        "--force",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "destroy",
        "CipherProductionState",
        "--force",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "destroy",
        "CipherProductionControl",
        "--force",
      ],
    ]);
  });

  test("keeps the complete dry-run plan scoped to four named stacks", () => {
    expect(
      plannedCommands("destroy-all", config)
        .flat()
        .every((value) => !value.includes("*")),
    ).toBe(true);
    expect(plannedCommands("resume", config)).toEqual([
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "diff",
        "CipherProductionState",
        "CipherProductionControl",
        "CipherProductionNetwork",
        "CipherProductionRuntime",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "exec",
        "cdk",
        "--",
        "deploy",
        "CipherProductionState",
        "CipherProductionControl",
        "CipherProductionNetwork",
        "CipherProductionRuntime",
        "--require-approval",
        "any-change",
      ],
    ]);
  });

  test("rejects an unsafe AWS target and failed infrastructure commands", () => {
    const { runner } = createRunner();
    expect(() =>
      runInfrastructureControl(
        ["pause", `--confirm=${confirmation("pause")}`],
        { isInteractive: true },
        runner,
        config,
      ),
    ).toThrow("expected 12-digit production account");

    const accountFailure: CommandRunner = {
      run() {
        return { exitCode: 1, stderr: "unavailable", stdout: "" };
      },
    };
    expect(() =>
      runInfrastructureControl(
        ["pause", `--confirm=${confirmation("pause")}`],
        interactiveEnvironment,
        accountFailure,
        config,
      ),
    ).toThrow("Could not verify the active AWS account");

    const unknownStack: CommandRunner = {
      run(command) {
        if (command[1] === "sts")
          return { exitCode: 0, stderr: "", stdout: `${expectedAccount}\n` };
        return { exitCode: 255, stderr: "access denied", stdout: "" };
      },
    };
    expect(() =>
      runInfrastructureControl(
        ["pause", `--confirm=${confirmation("pause")}`],
        interactiveEnvironment,
        unknownStack,
        config,
      ),
    ).toThrow("Could not determine whether CipherProductionRuntime exists");

    const commandFailure: CommandRunner = {
      run(command) {
        if (command[1] === "sts")
          return { exitCode: 0, stderr: "", stdout: `${expectedAccount}\n` };
        if (command[1] === "cloudformation") {
          return {
            exitCode: 0,
            stderr: "",
            stdout: command.includes("CipherProductionRuntime") ? "{}" : "",
          };
        }
        return { exitCode: 1, stderr: "failed", stdout: "" };
      },
    };
    expect(() =>
      runInfrastructureControl(
        ["pause", `--confirm=${confirmation("pause")}`],
        interactiveEnvironment,
        commandFailure,
        config,
      ),
    ).toThrow("Infrastructure command failed");
  });

  test("uses the native command runner without shell interpolation", () => {
    const result = liveRunner.run(["bun", "--version"]);
    expect(result.exitCode).toBe(0);
    expect(result.stdout.trim()).toMatch(/^1\./u);
  });

  test("formats dry-run and completed lifecycle results", () => {
    const messages: string[] = [];
    logInfrastructureControlResult("pause", false, [], (message) => messages.push(message));
    logInfrastructureControlResult("resume", true, ["deploy CipherProductionRuntime"], (message) =>
      messages.push(message),
    );
    logInfrastructureControlResult(
      "destroy-all",
      false,
      ["destroy CipherProductionRuntime"],
      (message) => messages.push(message),
    );

    expect(messages).toContain("No Cipher production stacks exist for this action.");
    expect(messages).toContain("Planned:");
    expect(messages).toContain("Completed:");
    expect(messages.some((message) => message.includes("storage costs"))).toBe(true);
    expect(messages.some((message) => message.includes("retention can still apply"))).toBe(true);
  });
});
