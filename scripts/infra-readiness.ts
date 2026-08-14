const productionRegion = "us-east-1";
const bootstrapStack = "CDKToolkit";

type StackName =
  | "CipherProductionState"
  | "CipherProductionControl"
  | "CipherProductionNetwork"
  | "CipherProductionRuntime";

export interface CommandResult {
  exitCode: number;
  stderr: string;
  stdout: string;
}

export interface CommandRunner {
  run(command: string[]): CommandResult;
}

export interface TemplateReader {
  read(path: string): Promise<unknown>;
}

export interface ReadinessEnvironment {
  accountId?: string;
}

const requiredResources: Record<StackName, string[]> = {
  CipherProductionState: ["AWS::Cognito::UserPool", "AWS::DynamoDB::Table", "AWS::S3::Bucket"],
  CipherProductionControl: ["AWS::ECR::Repository", "AWS::IAM::Role"],
  CipherProductionNetwork: ["AWS::EC2::VPC"],
  CipherProductionRuntime: ["AWS::ElasticLoadBalancingV2::LoadBalancer", "AWS::ECS::Service"],
};

const liveRunner: CommandRunner = {
  run(command) {
    const result = Bun.spawnSync(command, { stderr: "pipe", stdout: "pipe" });
    return {
      exitCode: result.exitCode,
      stderr: new TextDecoder().decode(result.stderr),
      stdout: new TextDecoder().decode(result.stdout),
    };
  },
};

const liveTemplateReader: TemplateReader = {
  async read(path) {
    try {
      return JSON.parse(await Bun.file(path).text()) as unknown;
    } catch {
      throw new Error(`Could not read synthesized template: ${path}.`);
    }
  },
};

function expectedAccount(environment: ReadinessEnvironment): string {
  if (environment.accountId === undefined || !/^\d{12}$/u.test(environment.accountId)) {
    throw new Error("CIPHER_AWS_ACCOUNT_ID must name the expected 12-digit production account.");
  }
  return environment.accountId;
}

function run(command: string[], runner: CommandRunner, failure: string): CommandResult {
  const result = runner.run(command);
  if (result.exitCode !== 0) {
    throw new Error(failure);
  }
  return result;
}

function assertActiveAccount(expected: string, runner: CommandRunner): void {
  const result = run(
    [
      "aws",
      "sts",
      "get-caller-identity",
      "--query",
      "Account",
      "--output",
      "text",
      "--region",
      productionRegion,
    ],
    runner,
    "Could not verify the active AWS account.",
  );
  if (result.stdout.trim() !== expected) {
    throw new Error("The active AWS account is not Cipher production.");
  }
}

function assertBootstrapReady(runner: CommandRunner): void {
  const result = run(
    [
      "aws",
      "cloudformation",
      "describe-stacks",
      "--stack-name",
      bootstrapStack,
      "--region",
      productionRegion,
      "--output",
      "json",
    ],
    runner,
    "CDK is not bootstrapped in us-east-1. Run CDK bootstrap before deploying Cipher.",
  );

  let stack: { EnableTerminationProtection?: unknown; StackStatus?: unknown } | undefined;
  try {
    const payload = JSON.parse(result.stdout) as {
      Stacks?: Array<{ EnableTerminationProtection?: unknown; StackStatus?: unknown }>;
    };
    stack = payload.Stacks?.[0];
  } catch {
    throw new Error("Could not read the CDK bootstrap stack status.");
  }
  if (stack?.EnableTerminationProtection !== true) {
    throw new Error("CDK bootstrap stack must have termination protection enabled.");
  }
  const status = stack.StackStatus;
  if (status !== "CREATE_COMPLETE" && status !== "UPDATE_COMPLETE") {
    throw new Error("The CDK bootstrap stack is not ready.");
  }
}

function synthesize(expected: string, runner: CommandRunner): void {
  const originalAccount = process.env.CDK_DEFAULT_ACCOUNT;
  const originalRegion = process.env.CIPHER_AWS_REGION;
  process.env.CDK_DEFAULT_ACCOUNT = expected;
  process.env.CIPHER_AWS_REGION = productionRegion;
  try {
    run(
      ["npm", "--prefix", "infra", "exec", "cdk", "--", "synth"],
      runner,
      "Cipher infrastructure could not be synthesized.",
    );
  } finally {
    if (originalAccount === undefined) {
      delete process.env.CDK_DEFAULT_ACCOUNT;
    } else {
      process.env.CDK_DEFAULT_ACCOUNT = originalAccount;
    }
    if (originalRegion === undefined) {
      delete process.env.CIPHER_AWS_REGION;
    } else {
      process.env.CIPHER_AWS_REGION = originalRegion;
    }
  }
}

function resourceTypes(template: unknown): Set<string> {
  if (typeof template !== "object" || template === null || !("Resources" in template)) {
    throw new Error("A synthesized template does not contain resources.");
  }
  const resources = template.Resources;
  if (typeof resources !== "object" || resources === null) {
    throw new Error("A synthesized template has invalid resources.");
  }

  const types = new Set<string>();
  for (const resource of Object.values(resources)) {
    if (typeof resource !== "object" || resource === null || !("Type" in resource)) {
      continue;
    }
    if (typeof resource.Type === "string") {
      types.add(resource.Type);
    }
  }
  return types;
}

async function assertStackShape(reader: TemplateReader): Promise<void> {
  for (const [stack, requirements] of Object.entries(requiredResources) as [
    StackName,
    string[],
  ][]) {
    const types = resourceTypes(await reader.read(`infra/cdk.out/${stack}.template.json`));
    const missing = requirements.filter((requirement) => !types.has(requirement));
    if (missing.length > 0) {
      throw new Error(`${stack} is not ready. Missing: ${missing.join(", ")}.`);
    }
  }
}

export async function runReadinessCheck(
  environment: ReadinessEnvironment,
  runner: CommandRunner,
  reader: TemplateReader,
): Promise<string[]> {
  const account = expectedAccount(environment);
  assertActiveAccount(account, runner);
  assertBootstrapReady(runner);
  synthesize(account, runner);
  await assertStackShape(reader);

  return [
    `AWS account ${account} and ${productionRegion} are ready.`,
    "CDK bootstrap is ready.",
    "All required Cipher stack resources are present in the synthesized templates.",
  ];
}

if (import.meta.main) {
  try {
    const results = await runReadinessCheck(
      { accountId: process.env.CIPHER_AWS_ACCOUNT_ID },
      liveRunner,
      liveTemplateReader,
    );
    console.log("Ready for deployment:");
    for (const result of results) {
      console.log(`- ${result}`);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : "Unknown readiness check failure.";
    console.error(`Not ready: ${message}`);
    process.exitCode = 1;
  }
}
