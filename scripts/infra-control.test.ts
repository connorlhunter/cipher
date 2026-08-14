import { describe, expect, test } from "bun:test";

import {
  parseArguments,
  plannedCommands,
  runInfrastructureControl,
  type CommandRunner,
} from "./infra-control";

const expectedAccount = "629577972102";

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
        "never",
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
      plannedCommands("destroy-all")
        .flat()
        .every((value) => !value.includes("*")),
    ).toBe(true);
    expect(plannedCommands("resume")).toEqual([
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
        "never",
      ],
    ]);
  });
});
