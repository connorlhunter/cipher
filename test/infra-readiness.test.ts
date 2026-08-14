import { describe, expect, test } from "bun:test";

import { loadInfrastructureConfig } from "../config/environment";
import {
  runReadinessCheck,
  type CommandRunner,
  type TemplateReader,
} from "../scripts/infra-readiness";

const accountId = "123456789012";
const config = loadInfrastructureConfig({
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_STATE_STACK: "CipherProductionState",
  CIPHER_CONTROL_STACK: "CipherProductionControl",
  CIPHER_NETWORK_STACK: "CipherProductionNetwork",
  CIPHER_RUNTIME_STACK: "CipherProductionRuntime",
});

const completeTemplates: Record<string, unknown> = {
  "infra/cdk.out/CipherProductionState.template.json": {
    Resources: {
      UserPool: { Type: "AWS::Cognito::UserPool" },
      Users: { Type: "AWS::DynamoDB::Table" },
      Media: { Type: "AWS::S3::Bucket" },
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
        "All required Cipher stack resources are present in the synthesized templates.",
      ],
    );

    expect(calls.map((command) => command.slice(0, 3))).toEqual([
      ["aws", "sts", "get-caller-identity"],
      ["aws", "cloudformation", "describe-stacks"],
      ["npm", "--prefix", "infra"],
    ]);
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
});
