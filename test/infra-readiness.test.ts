import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

import { loadInfrastructureConfig } from "../config/environment";
import {
  runReadinessCheck,
  liveRunner,
  liveTemplateReader,
  runReadinessCli,
  type CommandRunner,
  type TemplateReader,
} from "../scripts/infra-readiness";

const accountId = "123456789012";
const config = loadInfrastructureConfig({
  CIPHER_ACM_CERTIFICATE_ARN:
    "arn:aws:acm:us-east-1:123456789012:certificate/00000000-0000-4000-8000-000000000000",
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_STATE_STACK: "CipherProductionState",
  CIPHER_CONTROL_STACK: "CipherProductionControl",
  CIPHER_HOSTED_ZONE_ID: "Z000000000000000000000",
  CIPHER_NETWORK_STACK: "CipherProductionNetwork",
  CIPHER_RUNTIME_STACK: "CipherProductionRuntime",
});

const completeTemplates: Record<string, unknown> = {
  "infra/cdk.out/CipherProductionState.template.json": {
    Resources: {
      UserPool: { Type: "AWS::Cognito::UserPool" },
      Users: { Type: "AWS::DynamoDB::Table" },
      Conversations: { Type: "AWS::DynamoDB::Table" },
      Messages: { Type: "AWS::DynamoDB::Table" },
      MediaTable: { Type: "AWS::DynamoDB::Table" },
      Media: { Type: "AWS::S3::Bucket" },
    },
    Outputs: {
      CognitoUserPoolId: {},
      CognitoUserPoolClientId: {},
      UsersTableName: {},
      ConversationsTableName: {},
      MessagesTableName: {},
      MediaTableName: {},
      MediaBucketName: {},
    },
  },
  "infra/cdk.out/CipherProductionControl.template.json": {
    Resources: {
      Repository: { Type: "AWS::ECR::Repository" },
      DeploymentRole: { Type: "AWS::IAM::Role" },
    },
  },
  "infra/cdk.out/CipherProductionNetwork.template.json": {
    Resources: { Network: { Type: "AWS::EC2::VPC" } },
  },
  "infra/cdk.out/CipherProductionRuntime.template.json": {
    Resources: {
      LoadBalancer: { Type: "AWS::ElasticLoadBalancingV2::LoadBalancer" },
      Service: { Type: "AWS::ECS::Service" },
    },
  },
};

function createRunner(options?: {
  account?: string;
  bootstrapProtection?: boolean;
  bootstrapStatus?: string;
}): {
  calls: string[][];
  runner: CommandRunner;
} {
  const calls: string[][] = [];
  return {
    calls,
    runner: {
      run(command) {
        calls.push(command);
        if (command[0] === "aws" && command[1] === "sts") {
          return { exitCode: 0, stderr: "", stdout: `${options?.account ?? accountId}\n` };
        }
        if (command[0] === "aws" && command[1] === "cloudformation") {
          return {
            exitCode: 0,
            stderr: "",
            stdout: JSON.stringify({
              Stacks: [
                {
                  EnableTerminationProtection: options?.bootstrapProtection ?? true,
                  StackStatus: options?.bootstrapStatus ?? "CREATE_COMPLETE",
                },
              ],
            }),
          };
        }
        return { exitCode: 0, stderr: "", stdout: "" };
      },
    },
  };
}

function createReader(templates = completeTemplates): TemplateReader {
  return {
    async read(path) {
      const template = templates[path];
      if (template === undefined) {
        throw new Error(`Missing template: ${path}`);
      }
      return template;
    },
  };
}

describe("infrastructure readiness", () => {
  test("passes only when account, bootstrap, synthesis, and stack shape are ready", async () => {
    const { calls, runner } = createRunner();

    await expect(runReadinessCheck({ accountId }, runner, createReader(), config)).resolves.toEqual(
      [
        "AWS account 123456789012 and us-east-1 are ready.",
        "CDK bootstrap is ready.",
        "All required Cipher stack contracts are satisfied in the synthesized templates.",
      ],
    );

    expect(calls.map((command) => command.slice(0, 3))).toEqual([
      ["aws", "sts", "get-caller-identity"],
      ["aws", "cloudformation", "describe-stacks"],
      ["npm", "--prefix", "infra"],
    ]);
    expect(calls[2]).toEqual(["npm", "--prefix", "infra", "run", "synth"]);
  });

  test("stops before synthesis when the active account is wrong", async () => {
    const { calls, runner } = createRunner({ account: "000000000000" });

    await expect(runReadinessCheck({ accountId }, runner, createReader(), config)).rejects.toThrow(
      "active AWS account is not Cipher production",
    );
    expect(calls).toHaveLength(1);
  });

  test("stops before synthesis when CDK bootstrap is not ready", async () => {
    const { calls, runner } = createRunner({ bootstrapStatus: "UPDATE_IN_PROGRESS" });

    await expect(runReadinessCheck({ accountId }, runner, createReader(), config)).rejects.toThrow(
      "CDK bootstrap stack is not ready",
    );
    expect(calls).toHaveLength(2);
  });

  test("requires termination protection on CDK bootstrap", async () => {
    const { runner } = createRunner({ bootstrapProtection: false });

    await expect(runReadinessCheck({ accountId }, runner, createReader(), config)).rejects.toThrow(
      "CDK bootstrap stack must have termination protection enabled",
    );
  });

  test("rejects an incomplete runtime stack", async () => {
    const incompleteTemplates = structuredClone(completeTemplates);
    const runtime = incompleteTemplates["infra/cdk.out/CipherProductionRuntime.template.json"] as {
      Resources: Record<string, unknown>;
    };
    delete runtime.Resources.Service;
    const { runner } = createRunner();

    await expect(
      runReadinessCheck({ accountId }, runner, createReader(incompleteTemplates), config),
    ).rejects.toThrow("CipherProductionRuntime is not ready. Missing: AWS::ECS::Service.");
  });

  test("requires the exact state resource counts", async () => {
    const incompleteTemplates = structuredClone(completeTemplates);
    const state = incompleteTemplates["infra/cdk.out/CipherProductionState.template.json"] as {
      Resources: Record<string, unknown>;
    };
    state.Resources.ExtraTable = { Type: "AWS::DynamoDB::Table" };
    const { runner } = createRunner();

    await expect(
      runReadinessCheck({ accountId }, runner, createReader(incompleteTemplates), config),
    ).rejects.toThrow(
      "CipherProductionState is not ready. State resource counts must match: AWS::DynamoDB::Table: expected 4, found 5.",
    );
  });

  test("requires the state outputs used for runtime configuration", async () => {
    const incompleteTemplates = structuredClone(completeTemplates);
    const state = incompleteTemplates["infra/cdk.out/CipherProductionState.template.json"] as {
      Outputs: Record<string, unknown>;
    };
    delete state.Outputs.MediaBucketName;
    const { runner } = createRunner();

    await expect(
      runReadinessCheck({ accountId }, runner, createReader(incompleteTemplates), config),
    ).rejects.toThrow("CipherProductionState is not ready. Missing outputs: MediaBucketName.");
  });

  test("rejects malformed environments, failed commands, and malformed templates", async () => {
    const { runner } = createRunner();
    await expect(runReadinessCheck({}, runner, createReader(), config)).rejects.toThrow(
      "expected 12-digit production account",
    );

    const failedRunner: CommandRunner = {
      run() {
        return { exitCode: 1, stderr: "failed", stdout: "" };
      },
    };
    await expect(
      runReadinessCheck({ accountId }, failedRunner, createReader(), config),
    ).rejects.toThrow("Could not verify the active AWS account");

    const badResources = structuredClone(completeTemplates);
    badResources["infra/cdk.out/CipherProductionState.template.json"] = { Resources: "invalid" };
    await expect(
      runReadinessCheck({ accountId }, runner, createReader(badResources), config),
    ).rejects.toThrow("synthesized template has invalid resources");

    const missingOutputs = structuredClone(completeTemplates);
    const state = missingOutputs["infra/cdk.out/CipherProductionState.template.json"] as {
      Outputs?: Record<string, unknown>;
    };
    delete state.Outputs;
    await expect(
      runReadinessCheck({ accountId }, runner, createReader(missingOutputs), config),
    ).rejects.toThrow("synthesized template does not contain outputs");
  });

  test("uses native command and template readers without shell interpolation", async () => {
    const directory = mkdtempSync(join(tmpdir(), "cipher-readiness-adapter-"));
    try {
      expect(liveRunner.run(["bun", "--version"]).exitCode).toBe(0);
      const template = join(directory, "template.json");
      writeFileSync(template, '{"Resources":{}}');
      await expect(liveTemplateReader.read(template)).resolves.toEqual({ Resources: {} });
      writeFileSync(template, "invalid json");
      await expect(liveTemplateReader.read(template)).rejects.toThrow(
        "Could not read synthesized template",
      );
    } finally {
      rmSync(directory, { force: true, recursive: true });
    }
  });

  test("formats successful readiness checks for the CLI", async () => {
    const messages: string[] = [];
    const { runner } = createRunner();
    await expect(
      runReadinessCli(
        {
          CIPHER_AWS_ACCOUNT_ID: accountId,
          CIPHER_ACM_CERTIFICATE_ARN:
            "arn:aws:acm:us-east-1:123456789012:certificate/00000000-0000-4000-8000-000000000000",
          CIPHER_AWS_REGION: "us-east-1",
          CIPHER_CONTROL_STACK: "CipherProductionControl",
          CIPHER_HOSTED_ZONE_ID: "Z000000000000000000000",
          CIPHER_NETWORK_STACK: "CipherProductionNetwork",
          CIPHER_RUNTIME_STACK: "CipherProductionRuntime",
          CIPHER_STATE_STACK: "CipherProductionState",
        },
        runner,
        createReader(),
        (message) => messages.push(message),
      ),
    ).resolves.toHaveLength(3);
    expect(messages[0]).toBe("Ready for deployment:");
    expect(messages).toHaveLength(4);
  });
});
