const productionRegion = "us-east-1";
const stateStack = "CipherProductionState";
const controlStack = "CipherProductionControl";
const networkStack = "CipherProductionNetwork";
const runtimeStack = "CipherProductionRuntime";
const persistentStacks = [stateStack, controlStack];
const disposableStacks = [runtimeStack, networkStack];

export type InfrastructureAction = "pause" | "resume" | "destroy-all";

export interface CommandResult {
  exitCode: number;
  stderr: string;
  stdout: string;
}

export interface CommandRunner {
  run(command: string[]): CommandResult;
}

export interface InfrastructureEnvironment {
  accountId?: string;
  isInteractive?: boolean;
}

interface ParsedArguments {
  action: InfrastructureAction;
  confirmation?: string;
  destroyConfirmation?: string;
  dryRun: boolean;
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

function cdkCommand(...args: string[]): string[] {
  return ["npm", "--prefix", "infra", "exec", "cdk", "--", ...args];
}

export function plannedCommands(action: InfrastructureAction): string[][] {
  switch (action) {
    case "pause":
      return disposableStacks.map((stack) => cdkCommand("destroy", stack, "--force"));
    case "resume":
      return [
        cdkCommand(
          "deploy",
          stateStack,
          controlStack,
          networkStack,
          runtimeStack,
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

function accountId(environment: InfrastructureEnvironment): string {
  if (environment.accountId === undefined || !/^\d{12}$/u.test(environment.accountId)) {
    throw new Error("CIPHER_AWS_ACCOUNT_ID must name the expected 12-digit production account.");
  }
  return environment.accountId;
}

function confirmationFor(action: InfrastructureAction, expectedAccount: string): string {
  switch (action) {
    case "pause":
      return `PAUSE-CIPHER-PRODUCTION-${expectedAccount}-${productionRegion}`;
    case "resume":
      return `RESUME-CIPHER-PRODUCTION-${expectedAccount}-${productionRegion}`;
    case "destroy-all":
      return `UNLOCK-CIPHER-PRODUCTION-${expectedAccount}-${productionRegion}`;
  }
}

function destroyConfirmationFor(expectedAccount: string): string {
  return `DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA-${expectedAccount}-${productionRegion}`;
}

function currentAccount(runner: CommandRunner): string {
  const result = runner.run([
    "aws",
    "sts",
    "get-caller-identity",
    "--query",
    "Account",
    "--output",
    "text",
    "--region",
    productionRegion,
  ]);
  if (result.exitCode !== 0) {
    throw new Error("Could not verify the active AWS account before changing infrastructure.");
  }
  return result.stdout.trim();
}

function assertProductionTarget(
  environment: InfrastructureEnvironment,
  runner: CommandRunner,
): string {
  const expectedAccount = accountId(environment);
  const isInteractive = environment.isInteractive ?? (process.stdin.isTTY && process.stdout.isTTY);
  if (!isInteractive) {
    throw new Error("Refusing to change infrastructure outside an interactive terminal.");
  }
  if (currentAccount(runner) !== expectedAccount) {
    throw new Error("The active AWS account is not Cipher production. No changes were made.");
  }
  return expectedAccount;
}

function stackExists(stack: string, runner: CommandRunner): boolean {
  const result = runner.run([
    "aws",
    "cloudformation",
    "describe-stacks",
    "--stack-name",
    stack,
    "--region",
    productionRegion,
  ]);
  if (result.exitCode === 0) {
    return true;
  }
  if (`${result.stdout}${result.stderr}`.includes("does not exist")) {
    return false;
  }
  throw new Error(`Could not determine whether ${stack} exists. No changes were made.`);
}

function existingCommands(action: InfrastructureAction, runner: CommandRunner): string[][] {
  switch (action) {
    case "pause":
      return disposableStacks
        .filter((stack) => stackExists(stack, runner))
        .map((stack) => cdkCommand("destroy", stack, "--force"));
    case "resume":
      return plannedCommands(action);
    case "destroy-all": {
      const existingPersistentStacks = persistentStacks.filter((stack) =>
        stackExists(stack, runner),
      );
      const existingDisposableStacks = disposableStacks.filter((stack) =>
        stackExists(stack, runner),
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

function runCommand(command: string[], runner: CommandRunner): void {
  const result = runner.run(command);
  if (result.exitCode !== 0) {
    throw new Error(`Infrastructure command failed: ${command.join(" ")}`);
  }
}

export function runInfrastructureControl(
  args: string[],
  environment: InfrastructureEnvironment,
  runner: CommandRunner,
): string[] {
  const { action, confirmation, destroyConfirmation, dryRun } = parseArguments(args);
  const expectedAccount = accountId(environment);
  const expectedConfirmation = confirmationFor(action, expectedAccount);
  if (confirmation !== expectedConfirmation) {
    throw new Error(`Refusing ${action}: pass --confirm=${expectedConfirmation}.`);
  }
  if (action === "destroy-all") {
    const expectedDestroyConfirmation = destroyConfirmationFor(expectedAccount);
    if (destroyConfirmation !== expectedDestroyConfirmation) {
      throw new Error(
        `Refusing destroy-all: pass --destroy-confirm=${expectedDestroyConfirmation}.`,
      );
    }
  }

  if (dryRun) {
    return plannedCommands(action).map((command) => command.join(" "));
  }

  assertProductionTarget(environment, runner);
  const commands = existingCommands(action, runner);
  for (const command of commands) {
    runCommand(command, runner);
  }

  return commands.map((command) => command.join(" "));
}

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

if (import.meta.main) {
  const parsed = parseArguments(Bun.argv.slice(2));
  const commands = runInfrastructureControl(
    Bun.argv.slice(2),
    { accountId: process.env.CIPHER_AWS_ACCOUNT_ID },
    liveRunner,
  );

  if (commands.length === 0) {
    console.log("No Cipher production stacks exist for this action.");
  } else {
    console.log(parsed.dryRun ? "Planned:" : "Completed:");
    for (const command of commands) {
      console.log(`- ${command}`);
    }
  }
  console.log(
    parsed.dryRun ? "Dry run only. No AWS commands were run." : completionNote(parsed.action),
  );
}
