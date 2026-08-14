import { type InfrastructureConfig, loadInfrastructureConfig } from "../config/environment";

const bootstrapStack = "CDKToolkit";

/**
 * @property exitCode - Child-process exit code.
 * @property stderr - Captured standard error.
 * @property stdout - Captured standard output.
 */
export interface CommandResult {
  exitCode: number;
  stderr: string;
  stdout: string;
}

/**
 * @property run - Executes a command and returns its captured result.
 */
export interface CommandRunner {
  run(command: string[]): CommandResult;
}

/**
 * @property read - Reads and parses a synthesized CloudFormation template.
 */
export interface TemplateReader {
  read(path: string): Promise<unknown>;
}

/**
 * @property accountId - Expected production AWS account identifier.
 */
export interface ReadinessEnvironment {
  accountId?: string;
}

/**
 * Resource types required in each synthesized production stack.
 *
 * @param config - Validated deployment configuration.
 * @returns Required CloudFormation resource types by stack.
 */
function requiredResources(config: InfrastructureConfig): Map<string, ReadonlyArray<string>> {
  return new Map<string, ReadonlyArray<string>>([
    [config.stacks.state, ["AWS::Cognito::UserPool", "AWS::DynamoDB::Table", "AWS::S3::Bucket"]],
    [config.stacks.control, ["AWS::ECR::Repository", "AWS::IAM::Role"]],
    [config.stacks.network, ["AWS::EC2::VPC"]],
    [config.stacks.runtime, ["AWS::ElasticLoadBalancingV2::LoadBalancer", "AWS::ECS::Service"]],
  ]);
}

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

/**
 * @param environment - Readiness environment to validate.
 * @returns The expected 12-digit production account identifier.
 */
function expectedAccount(environment: ReadinessEnvironment): string {
  if (environment.accountId === undefined || !/^\d{12}$/u.test(environment.accountId)) {
    throw new Error("CIPHER_AWS_ACCOUNT_ID must name the expected 12-digit production account.");
  }
  return environment.accountId;
}

/**
 * @param command - Command to execute.
 * @param runner - Command runner used for execution.
 * @param failure - Error message used when the command fails.
 * @returns The successful command result.
 */
function run(command: string[], runner: CommandRunner, failure: string): CommandResult {
  const result = runner.run(command);
  if (result.exitCode !== 0) {
    throw new Error(failure);
  }
  return result;
}

/**
 * @param expected - Expected production AWS account identifier.
 * @param runner - Command runner used to query AWS.
 * @returns Nothing; throws when the active account differs.
 */
function assertActiveAccount(
  expected: string,
  runner: CommandRunner,
  config: InfrastructureConfig,
): void {
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
      config.awsRegion,
    ],
    runner,
    "Could not verify the active AWS account.",
  );
  if (result.stdout.trim() !== expected) {
    throw new Error("The active AWS account is not Cipher production.");
  }
}

/**
 * @param runner - Command runner used to inspect the CDK bootstrap stack.
 * @returns Nothing; throws when bootstrap is absent, changing, or unprotected.
 */
function assertBootstrapReady(runner: CommandRunner, config: InfrastructureConfig): void {
  const result = run(
    [
      "aws",
      "cloudformation",
      "describe-stacks",
      "--stack-name",
      bootstrapStack,
      "--region",
      config.awsRegion,
      "--output",
      "json",
    ],
    runner,
    `CDK is not bootstrapped in ${config.awsRegion}. Run CDK bootstrap before deploying Cipher.`,
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

/**
 * @param expected - Expected production AWS account identifier.
 * @param runner - Command runner used to synthesize the infrastructure.
 * @returns Nothing; restores the caller's environment after synthesis.
 */
function synthesize(expected: string, runner: CommandRunner, config: InfrastructureConfig): void {
  const originalAccount = process.env.CDK_DEFAULT_ACCOUNT;
  const originalRegion = process.env.CIPHER_AWS_REGION;
  process.env.CDK_DEFAULT_ACCOUNT = expected;
  process.env.CIPHER_AWS_REGION = config.awsRegion;
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

/**
 * @param template - Synthesized CloudFormation template to inspect.
 * @returns Resource types declared by the template.
 */
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

/**
 * @param reader - Reader for synthesized CloudFormation templates.
 * @returns Nothing; throws when a required stack resource is absent.
 */
async function assertStackShape(
  reader: TemplateReader,
  config: InfrastructureConfig,
): Promise<void> {
  for (const [stack, requirements] of requiredResources(config)) {
    const types = resourceTypes(await reader.read(`infra/cdk.out/${stack}.template.json`));
    const missing = requirements.filter((requirement) => !types.has(requirement));
    if (missing.length > 0) {
      throw new Error(`${stack} is not ready. Missing: ${missing.join(", ")}.`);
    }
  }
}

/**
 * Verifies the AWS target, CDK bootstrap, synthesis, and required stack resources.
 *
 * @param environment - Expected production environment.
 * @param runner - Command runner used for AWS and CDK operations.
 * @param reader - Reader for synthesized CloudFormation templates.
 * @param config - Validated deployment configuration.
 * @returns Display-ready readiness results.
 */
export async function runReadinessCheck(
  environment: ReadinessEnvironment,
  runner: CommandRunner,
  reader: TemplateReader,
  config: InfrastructureConfig,
): Promise<string[]> {
  const account = expectedAccount(environment);
  assertActiveAccount(account, runner, config);
  assertBootstrapReady(runner, config);
  synthesize(account, runner, config);
  await assertStackShape(reader, config);

  return [
    `AWS account ${account} and ${config.awsRegion} are ready.`,
    "CDK bootstrap is ready.",
    "All required Cipher stack resources are present in the synthesized templates.",
  ];
}

if (import.meta.main) {
  try {
    const config = loadInfrastructureConfig(process.env as Record<string, string | undefined>);
    const results = await runReadinessCheck(
      { accountId: process.env.CIPHER_AWS_ACCOUNT_ID },
      liveRunner,
      liveTemplateReader,
      config,
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
