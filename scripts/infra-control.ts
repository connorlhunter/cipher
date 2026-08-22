import { type InfrastructureConfig, loadInfrastructureConfig } from "../config/environment";

const productionBackupVaultName = "cipher-production-recovery";

/**
 * Infrastructure lifecycle actions supported by the production control script.
 */
export type InfrastructureAction = "pause" | "resume" | "destroy-all";

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
 * @property accountId - Expected production AWS account identifier.
 * @property isInteractive - Optional terminal-interactivity override.
 */
export interface InfrastructureEnvironment {
  accountId?: string;
  isInteractive?: boolean;
}

/**
 * Parsed command-line values for one infrastructure action.
 */
interface ParsedArguments {
  action: InfrastructureAction;
  confirmation?: string;
  destroyConfirmation?: string;
  dryRun: boolean;
  imageTag?: string;
}

/**
 * Runs CDK in the operator's terminal so the change plan and approval prompt
 * remain visible; account checks keep their output captured for validation.
 */
export const liveRunner: CommandRunner = {
  run(command) {
    const isCdkCommand = command[0] === "npm" && command.includes("cdk");
    const result = Bun.spawnSync(command, {
      stderr: isCdkCommand ? "inherit" : "pipe",
      stdin: isCdkCommand ? "inherit" : "ignore",
      stdout: isCdkCommand ? "inherit" : "pipe",
    });
    return {
      exitCode: result.exitCode,
      stderr: decodeOutput(result.stderr),
      stdout: decodeOutput(result.stdout),
    };
  },
};

/** @returns Decoded captured output, or an empty value when the stream was inherited. */
function decodeOutput(output: Uint8Array | undefined): string {
  return output === undefined ? "" : new TextDecoder().decode(output);
}

/**
 * @param args - Command-line arguments to parse.
 * @returns A validated infrastructure action and its safety flags.
 */
export function parseArguments(args: string[]): ParsedArguments {
  const [action, ...flags] = args;
  if (action !== "pause" && action !== "resume" && action !== "destroy-all") {
    throw new Error("Choose one action: pause, resume, or destroy-all.");
  }

  let confirmation: string | undefined;
  let destroyConfirmation: string | undefined;
  let dryRun = false;
  let imageTag: string | undefined;
  for (const flag of flags) {
    if (flag === "--dry-run") {
      dryRun = true;
      continue;
    }
    if (flag.startsWith("--confirm=")) {
      confirmation = flag.slice("--confirm=".length);
      continue;
    }
    if (flag.startsWith("--destroy-confirm=")) {
      destroyConfirmation = flag.slice("--destroy-confirm=".length);
      continue;
    }
    if (flag.startsWith("--image-tag=")) {
      imageTag = flag.slice("--image-tag=".length);
      continue;
    }
    throw new Error(`Unknown infrastructure control option: ${flag}`);
  }

  if (imageTag !== undefined && !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/u.test(imageTag)) {
    throw new Error("--image-tag must be one immutable ECR tag value.");
  }
  if (action === "resume" && imageTag === undefined) {
    throw new Error("Resume requires --image-tag=<immutable-server-image-tag>.");
  }
  if (action !== "resume" && imageTag !== undefined) {
    throw new Error("--image-tag is supported only for resume.");
  }

  return { action, confirmation, destroyConfirmation, dryRun, imageTag };
}

/**
 * @param args - CDK arguments to append after the infrastructure package script separator.
 * @returns A command that runs the infrastructure package's CDK script.
 */
function cdkCommand(...args: string[]): string[] {
  return ["npm", "--prefix", "infra", "run", "cdk", "--", ...args];
}

/**
 * @param action - Infrastructure lifecycle action to plan.
 * @param config - Validated deployment configuration.
 * @returns The complete ordered command plan for the action.
 */
export function plannedCommands(
  action: InfrastructureAction,
  config: InfrastructureConfig,
  imageTag?: string,
): string[][] {
  const persistentDeployStacks = [config.stacks.state, config.stacks.control];
  const persistentDestroyStacks = [config.stacks.control, config.stacks.state];
  const disposableStacks = [config.stacks.runtime, config.stacks.network];
  switch (action) {
    case "pause":
      return disposableStacks.map((stack) => cdkCommand("destroy", stack, "--force"));
    case "resume": {
      if (imageTag === undefined) {
        throw new Error("Resume requires --image-tag=<immutable-server-image-tag>.");
      }
      const imageParameter = `${config.stacks.runtime}:ServerImageTag=${imageTag}`;
      return [
        ["bun", "run", "infra:readiness"],
        cdkCommand(
          "diff",
          ...persistentDeployStacks,
          config.stacks.network,
          config.stacks.runtime,
          "--parameters",
          imageParameter,
        ),
        cdkCommand(
          "deploy",
          ...persistentDeployStacks,
          config.stacks.network,
          config.stacks.runtime,
          "--parameters",
          imageParameter,
          "--require-approval",
          "any-change",
        ),
      ];
    }
    case "destroy-all":
      return [
        cdkCommand(
          "deploy",
          ...persistentDeployStacks,
          "--context",
          "cipher:allow-persistent-destruction=true",
          "--require-approval",
          "any-change",
        ),
        ...disposableStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
        ...persistentDestroyStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
      ];
  }
}

/**
 * @param environment - Infrastructure environment to validate.
 * @returns The expected 12-digit production account identifier.
 */
function accountId(environment: InfrastructureEnvironment): string {
  if (environment.accountId === undefined || !/^\d{12}$/u.test(environment.accountId)) {
    throw new Error("CIPHER_AWS_ACCOUNT_ID must name the expected 12-digit production account.");
  }
  return environment.accountId;
}

/**
 * @param action - Infrastructure action requiring confirmation.
 * @param expectedAccount - Expected production AWS account identifier.
 * @returns The action-specific confirmation phrase.
 */
function confirmationFor(
  action: InfrastructureAction,
  expectedAccount: string,
  config: InfrastructureConfig,
): string {
  switch (action) {
    case "pause":
      return `PAUSE-CIPHER-PRODUCTION-${expectedAccount}-${config.awsRegion}`;
    case "resume":
      return `RESUME-CIPHER-PRODUCTION-${expectedAccount}-${config.awsRegion}`;
    case "destroy-all":
      return `UNLOCK-CIPHER-PRODUCTION-${expectedAccount}-${config.awsRegion}`;
  }
}

/**
 * @param expectedAccount - Expected production AWS account identifier.
 * @returns The additional irreversible-destruction confirmation phrase.
 */
function destroyConfirmationFor(expectedAccount: string, config: InfrastructureConfig): string {
  return `DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA-${expectedAccount}-${config.awsRegion}`;
}

/**
 * @param runner - Command runner used to query AWS.
 * @returns The active AWS account identifier.
 */
function currentAccount(runner: CommandRunner, config: InfrastructureConfig): string {
  const result = runner.run([
    "aws",
    "sts",
    "get-caller-identity",
    "--query",
    "Account",
    "--output",
    "text",
    "--region",
    config.awsRegion,
  ]);
  if (result.exitCode !== 0) {
    throw new Error("Could not verify the active AWS account before changing infrastructure.");
  }
  return result.stdout.trim();
}

/**
 * @param environment - Expected production environment and terminal state.
 * @param runner - Command runner used to query AWS.
 * @returns The verified production AWS account identifier.
 */
function assertProductionTarget(
  environment: InfrastructureEnvironment,
  runner: CommandRunner,
  config: InfrastructureConfig,
): string {
  const expectedAccount = accountId(environment);
  const isInteractive = environment.isInteractive ?? (process.stdin.isTTY && process.stdout.isTTY);
  if (!isInteractive) {
    throw new Error("Refusing to change infrastructure outside an interactive terminal.");
  }
  if (currentAccount(runner, config) !== expectedAccount) {
    throw new Error("The active AWS account is not Cipher production. No changes were made.");
  }
  return expectedAccount;
}

/**
 * @param stack - Exact CloudFormation stack name to inspect.
 * @param runner - Command runner used to query AWS.
 * @returns Whether the named stack currently exists.
 */
function stackExists(stack: string, runner: CommandRunner, config: InfrastructureConfig): boolean {
  const result = runner.run([
    "aws",
    "cloudformation",
    "describe-stacks",
    "--stack-name",
    stack,
    "--region",
    config.awsRegion,
  ]);
  if (result.exitCode === 0) {
    return true;
  }
  if (`${result.stdout}${result.stderr}`.includes("does not exist")) {
    return false;
  }
  throw new Error(`Could not determine whether ${stack} exists. No changes were made.`);
}

/**
 * @param action - Infrastructure action to reduce to existing stacks.
 * @param runner - Command runner used to query AWS.
 * @returns Commands that still need to run for the action.
 */
function existingCommands(
  action: InfrastructureAction,
  runner: CommandRunner,
  config: InfrastructureConfig,
  imageTag?: string,
): string[][] {
  const persistentDeployStacks = [config.stacks.state, config.stacks.control];
  const persistentDestroyStacks = [config.stacks.control, config.stacks.state];
  const disposableStacks = [config.stacks.runtime, config.stacks.network];
  switch (action) {
    case "pause":
      return disposableStacks
        .filter((stack) => stackExists(stack, runner, config))
        .map((stack) => cdkCommand("destroy", stack, "--force"));
    case "resume":
      return plannedCommands(action, config, imageTag);
    case "destroy-all": {
      const existingPersistentStacks = persistentDeployStacks.filter((stack) =>
        stackExists(stack, runner, config),
      );
      const existingPersistentStackSet = new Set(existingPersistentStacks);
      const existingDisposableStacks = disposableStacks.filter((stack) =>
        stackExists(stack, runner, config),
      );
      if (existingPersistentStacks.length === 0 && existingDisposableStacks.length === 0) {
        return [];
      }
      return [
        ...(existingPersistentStacks.length === 0
          ? []
          : [
              cdkCommand(
                "deploy",
                ...existingPersistentStacks,
                "--context",
                "cipher:allow-persistent-destruction=true",
                "--require-approval",
                "any-change",
              ),
            ]),
        ...existingDisposableStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
        ...persistentDestroyStacks
          .filter((stack) => existingPersistentStackSet.has(stack))
          .map((stack) => cdkCommand("destroy", stack, "--force")),
      ];
    }
  }
}

/**
 * @param command - Infrastructure command to execute.
 * @param runner - Command runner used for execution.
 * @returns Nothing; throws when the command fails.
 */
function runCommand(command: string[], runner: CommandRunner): void {
  const result = runner.run(command);
  if (result.exitCode !== 0) {
    throw new Error(`Infrastructure command failed: ${command.join(" ")}`);
  }
}

/**
 * @param recoveryPointArn - AWS Backup recovery point returned by the exact production vault.
 * @param expectedAccount - Expected production AWS account identifier.
 * @returns Whether the recovery point is scoped to the verified production account.
 */
function isProductionRecoveryPointArn(recoveryPointArn: string, expectedAccount: string): boolean {
  return new RegExp(`^arn:[^:\\s]+:[^:\\s]+:[^:\\s]*:${expectedAccount}:[^\\s]+$`, "u").test(
    recoveryPointArn,
  );
}

/**
 * Removes only recovery points currently returned by Cipher's exact production vault.
 *
 * The vault itself cannot be removed while recovery points remain. This runs only after
 * State and Control have been switched to destructive mode and both confirmations have
 * already been accepted.
 *
 * @param runner - Command runner used for AWS CLI operations.
 * @param config - Validated production deployment configuration.
 * @param expectedAccount - Verified production AWS account identifier.
 * @returns Number of recovery points removed from the exact vault.
 */
function deleteProductionRecoveryPoints(
  runner: CommandRunner,
  config: InfrastructureConfig,
  expectedAccount: string,
): number {
  const listResult = runner.run([
    "aws",
    "backup",
    "list-recovery-points-by-backup-vault",
    "--backup-vault-name",
    productionBackupVaultName,
    "--region",
    config.awsRegion,
    "--output",
    "json",
  ]);
  if (listResult.exitCode !== 0) {
    throw new Error("Could not list recovery points in Cipher's production backup vault.");
  }

  let parsed: { RecoveryPoints?: unknown };
  try {
    parsed = JSON.parse(listResult.stdout) as { RecoveryPoints?: unknown };
  } catch {
    throw new Error("Could not read recovery points in Cipher's production backup vault.");
  }
  if (parsed.RecoveryPoints === undefined) {
    return 0;
  }
  if (!Array.isArray(parsed.RecoveryPoints)) {
    throw new Error("Cipher's production backup vault returned an invalid recovery-point list.");
  }

  const recoveryPointArns = parsed.RecoveryPoints.map((recoveryPoint) => {
    if (typeof recoveryPoint !== "object" || recoveryPoint === null) {
      throw new Error("Cipher's production backup vault returned an invalid recovery point.");
    }
    const recoveryPointArn = (recoveryPoint as { RecoveryPointArn?: unknown }).RecoveryPointArn;
    if (
      typeof recoveryPointArn !== "string" ||
      !isProductionRecoveryPointArn(recoveryPointArn, expectedAccount)
    ) {
      throw new Error("Cipher's production backup vault returned an out-of-scope recovery point.");
    }
    return recoveryPointArn;
  });

  for (const recoveryPointArn of recoveryPointArns) {
    const deleteResult = runner.run([
      "aws",
      "backup",
      "delete-recovery-point",
      "--backup-vault-name",
      productionBackupVaultName,
      "--recovery-point-arn",
      recoveryPointArn,
      "--region",
      config.awsRegion,
    ]);
    if (deleteResult.exitCode !== 0) {
      throw new Error("Could not delete a recovery point from Cipher's production backup vault.");
    }
  }

  return recoveryPointArns.length;
}

/**
 * @param command - Planned infrastructure command.
 * @returns Whether the command switches State and Control into destructive mode.
 */
function isDestructivePreparation(command: string[]): boolean {
  return command.includes("cipher:allow-persistent-destruction=true");
}

/**
 * Validates safety confirmations and applies one production infrastructure action.
 *
 * @param args - Command-line arguments describing the action and confirmations.
 * @param environment - Expected production environment and terminal state.
 * @param runner - Command runner used for AWS and CDK operations.
 * @param config - Validated deployment configuration.
 * @returns Display-ready commands that were planned or completed.
 */
export function runInfrastructureControl(
  args: string[],
  environment: InfrastructureEnvironment,
  runner: CommandRunner,
  config: InfrastructureConfig,
): string[] {
  const { action, confirmation, destroyConfirmation, dryRun, imageTag } = parseArguments(args);
  const expectedAccount = accountId(environment);
  const expectedConfirmation = confirmationFor(action, expectedAccount, config);
  if (confirmation !== expectedConfirmation) {
    throw new Error(`Refusing ${action}: pass --confirm=${expectedConfirmation}.`);
  }
  if (action === "destroy-all") {
    const expectedDestroyConfirmation = destroyConfirmationFor(expectedAccount, config);
    if (destroyConfirmation !== expectedDestroyConfirmation) {
      throw new Error(
        `Refusing destroy-all: pass --destroy-confirm=${expectedDestroyConfirmation}.`,
      );
    }
  }

  if (dryRun) {
    return plannedCommands(action, config, imageTag).map((command) => command.join(" "));
  }

  const verifiedAccount = assertProductionTarget(environment, runner, config);
  const commands = existingCommands(action, runner, config, imageTag);
  const completed: string[] = [];
  for (const command of commands) {
    runCommand(command, runner);
    completed.push(command.join(" "));
    if (action === "destroy-all" && isDestructivePreparation(command)) {
      if (command.includes(config.stacks.control)) {
        const recoveryPointCount = deleteProductionRecoveryPoints(runner, config, verifiedAccount);
        completed.push(
          `Deleted ${recoveryPointCount} recovery point(s) from ${productionBackupVaultName}.`,
        );
      }
    }
  }

  return completed;
}

/**
 * @param action - Completed infrastructure action.
 * @returns A short operational note about the resulting state.
 */
function completionNote(action: InfrastructureAction): string {
  switch (action) {
    case "pause":
      return "State and control remain. Cognito, DynamoDB, S3, ECR, and retained logs can still have storage costs.";
    case "resume":
      return "The protected state and control stacks were kept in place while the network and runtime were restored.";
    case "destroy-all":
      return "The named stacks were destroyed after protected resources entered destructive mode. AWS provider-managed recovery can remain for a limited period; check the destruction receipt before closing the account.";
  }
}

/** Formats the control command result for a human operator. */
export function logInfrastructureControlResult(
  action: InfrastructureAction,
  dryRun: boolean,
  commands: ReadonlyArray<string>,
  log: (message: string) => void,
): void {
  if (commands.length === 0) {
    log("No Cipher production stacks exist for this action.");
  } else {
    log(dryRun ? "Planned:" : "Completed:");
    for (const command of commands) {
      log(`- ${command}`);
    }
  }
  log(dryRun ? "Dry run only. No AWS commands were run." : completionNote(action));
}

if (import.meta.main) {
  const parsed = parseArguments(Bun.argv.slice(2));
  const config = loadInfrastructureConfig(process.env as Record<string, string | undefined>);
  const commands = runInfrastructureControl(
    Bun.argv.slice(2),
    { accountId: process.env.CIPHER_AWS_ACCOUNT_ID },
    liveRunner,
    config,
  );

  logInfrastructureControlResult(parsed.action, parsed.dryRun, commands, console.log);
}
