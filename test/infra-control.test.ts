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
  CIPHER_ACM_CERTIFICATE_ARN:
    "arn:aws:acm:us-east-1:123456789012:certificate/00000000-0000-4000-8000-000000000000",
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_BUDGET_ALERT_EMAIL: "production-alerts@example.invalid",
  CIPHER_STATE_STACK: "CipherProductionState",
  CIPHER_CONTROL_STACK: "CipherProductionControl",
  CIPHER_HOSTED_ZONE_ID: "Z000000000000000000000",
  CIPHER_NETWORK_STACK: "CipherProductionNetwork",
  CIPHER_RUNTIME_STACK: "CipherProductionRuntime",
});

function confirmation(action: "pause" | "resume" | "destroy-all"): string {
  const verb = action === "destroy-all" ? "UNLOCK" : action.toUpperCase();
  return `${verb}-CIPHER-PRODUCTION-${expectedAccount}-us-east-1`;
}

function createRunner(options?: {
  account?: string;
  existingStacks?: string[];
  recoveryPoints?: string[];
}): {
  calls: string[][];
  runner: CommandRunner;
} {
  const account = options?.account ?? expectedAccount;
  const existingStacks = new Set(options?.existingStacks ?? []);
  const recoveryPoints = options?.recoveryPoints ?? [];
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
        if (command[0] === "aws" && command[1] === "backup") {
          if (command[2] === "list-recovery-points-by-backup-vault") {
            return {
              exitCode: 0,
              stderr: "",
              stdout: JSON.stringify({
                RecoveryPoints: recoveryPoints.map((RecoveryPointArn) => ({ RecoveryPointArn })),
              }),
            };
          }
          return { exitCode: 0, stderr: "", stdout: "" };
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
    expect(() => parseArguments(["resume"])).toThrow("Resume requires --image-tag");
    expect(() => parseArguments(["resume", "--image-tag=unsafe/tag"])).toThrow(
      "--image-tag must be one immutable ECR tag value",
    );
    expect(() => parseArguments(["pause", "--image-tag=release-20260822"])).toThrow(
      "--image-tag is supported only for resume",
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
      "npm --prefix infra run cdk -- destroy CipherProductionRuntime --force",
      "npm --prefix infra run cdk -- destroy CipherProductionNetwork --force",
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
        ["resume", `--confirm=${confirmation("resume")}`, "--image-tag=release-20260822"],
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
        ["resume", `--confirm=${confirmation("resume")}`, "--image-tag=release-20260822"],
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
        "run",
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
        "run",
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
        "run",
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
        "run",
        "cdk",
        "--",
        "destroy",
        "CipherProductionControl",
        "--force",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "run",
        "cdk",
        "--",
        "destroy",
        "CipherProductionState",
        "--force",
      ],
    ]);
    expect(calls).toContainEqual([
      "aws",
      "backup",
      "list-recovery-points-by-backup-vault",
      "--backup-vault-name",
      "cipher-production-recovery",
      "--region",
      "us-east-1",
      "--output",
      "json",
    ]);
  });

  test("clears only the exact production backup vault before control is destroyed", () => {
    const recoveryPointArn =
      "arn:aws:dynamodb:us-east-1:123456789012:table/cipher-production-users/backup/01771900000000-abcdef";
    const { calls, runner } = createRunner({
      existingStacks: ["CipherProductionControl"],
      recoveryPoints: [recoveryPointArn],
    });

    const completed = runInfrastructureControl(
      [
        "destroy-all",
        `--confirm=${confirmation("destroy-all")}`,
        `--destroy-confirm=DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA-${expectedAccount}-us-east-1`,
      ],
      interactiveEnvironment,
      runner,
      config,
    );

    expect(calls).toContainEqual([
      "aws",
      "backup",
      "delete-recovery-point",
      "--backup-vault-name",
      "cipher-production-recovery",
      "--recovery-point-arn",
      recoveryPointArn,
      "--region",
      "us-east-1",
    ]);
    expect(completed).toContain("Deleted 1 recovery point(s) from cipher-production-recovery.");
  });

  test("refuses an out-of-scope recovery point before deleting it", () => {
    const { calls, runner } = createRunner({
      existingStacks: ["CipherProductionControl"],
      recoveryPoints: [
        "arn:aws:dynamodb:us-east-1:000000000000:table/cipher-production-users/backup/unsafe",
      ],
    });

    expect(() =>
      runInfrastructureControl(
        [
          "destroy-all",
          `--confirm=${confirmation("destroy-all")}`,
          `--destroy-confirm=DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA-${expectedAccount}-us-east-1`,
        ],
        interactiveEnvironment,
        runner,
        config,
      ),
    ).toThrow("out-of-scope recovery point");
    expect(calls.some((command) => command[2] === "delete-recovery-point")).toBe(false);
  });

  test("keeps the complete dry-run plan scoped to four named stacks", () => {
    expect(
      plannedCommands("destroy-all", config)
        .flat()
        .every((value) => !value.includes("*")),
    ).toBe(true);
    expect(plannedCommands("resume", config, "release-20260822")).toEqual([
      ["bun", "run", "infra:readiness"],
      [
        "npm",
        "--prefix",
        "infra",
        "run",
        "cdk",
        "--",
        "diff",
        "CipherProductionState",
        "CipherProductionControl",
        "CipherProductionNetwork",
        "CipherProductionRuntime",
        "--parameters",
        "CipherProductionRuntime:ServerImageTag=release-20260822",
      ],
      [
        "npm",
        "--prefix",
        "infra",
        "run",
        "cdk",
        "--",
        "deploy",
        "CipherProductionState",
        "CipherProductionControl",
        "CipherProductionNetwork",
        "CipherProductionRuntime",
        "--parameters",
        "CipherProductionRuntime:ServerImageTag=release-20260822",
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
    expect(messages.some((message) => message.includes("provider-managed recovery"))).toBe(true);
  });
});
