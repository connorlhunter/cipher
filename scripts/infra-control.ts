import { type InfrastructureConfig, loadInfrastructureConfig } from "../config/environment";

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
}

export const liveRunner: CommandRunner = {
  run(command) {
    const result = Bun.spawnSync(command, { stderr: "pipe", stdout: "pipe" });
    return {
      exitCode: result.exitCode,
      stderr: new TextDecoder().decode(result.stderr),
      stdout: new TextDecoder().decode(result.stdout),
    };
  },
};

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
    throw new Error(`Unknown infrastructure control option: ${flag}`);
  }

  return { action, confirmation, destroyConfirmation, dryRun };
}

/**
 * @param args - CDK arguments to append after the package-local executable separator.
 * @returns A command that runs the infrastructure package's CDK executable.
 */
function cdkCommand(...args: string[]): string[] {
  return ["npm", "--prefix", "infra", "exec", "cdk", "--", ...args];
}

/**
 * @param action - Infrastructure lifecycle action to plan.
 * @param config - Validated deployment configuration.
 * @returns The complete ordered command plan for the action.
 */
export function plannedCommands(
  action: InfrastructureAction,
  config: InfrastructureConfig,
): string[][] {
  const persistentStacks = [config.stacks.state, config.stacks.control];
  const disposableStacks = [config.stacks.runtime, config.stacks.network];
  switch (action) {
    case "pause":
      return disposableStacks.map((stack) => cdkCommand("destroy", stack, "--force"));
    case "resume":
      return [
        cdkCommand(
          "deploy",
          config.stacks.state,
          config.stacks.control,
          config.stacks.network,
          config.stacks.runtime,
          "--require-approval",
          "never",
        ),
      ];
    case "destroy-all":
      return [
        cdkCommand(
          "deploy",
          ...persistentStacks,
          "--context",
          "cipher:allow-persistent-destruction=true",
          "--require-approval",
          "never",
        ),
        ...disposableStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
        ...persistentStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
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
): string[][] {
  const persistentStacks = [config.stacks.state, config.stacks.control];
  const disposableStacks = [config.stacks.runtime, config.stacks.network];
  switch (action) {
    case "pause":
      return disposableStacks
        .filter((stack) => stackExists(stack, runner, config))
        .map((stack) => cdkCommand("destroy", stack, "--force"));
    case "resume":
      return plannedCommands(action, config);
    case "destroy-all": {
      const existingPersistentStacks = persistentStacks.filter((stack) =>
        stackExists(stack, runner, config),
      );
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
                "never",
              ),
            ]),
        ...existingDisposableStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
        ...existingPersistentStacks.map((stack) => cdkCommand("destroy", stack, "--force")),
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
  const { action, confirmation, destroyConfirmation, dryRun } = parseArguments(args);
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
    return plannedCommands(action, config).map((command) => command.join(" "));
  }

  assertProductionTarget(environment, runner, config);
  const commands = existingCommands(action, runner, config);
  for (const command of commands) {
    runCommand(command, runner);
  }

  return commands.map((command) => command.join(" "));
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
      return "AWS-managed retention can still apply after deletion. Check the destruction receipt before closing the account.";
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
