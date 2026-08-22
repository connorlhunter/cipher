/** Environment variables used by deployment and infrastructure controls. */
export type Environment = Readonly<Record<string, string | undefined>>;

/** Exact CloudFormation stack names for one Cipher deployment. */
export interface InfrastructureConfig {
  awsRegion: string;
  budgetAlertEmail: string;
  certificateArn: string;
  hostedZoneId: string;
  runtimeSecretArn?: string;
  stacks: {
    state: string;
    control: string;
    network: string;
    runtime: string;
  };
}

/**
 * @param environment - Environment variables to validate.
 * @param key - Required variable name.
 * @returns A non-empty environment value.
 */
function required(environment: Environment, key: string): string {
  const value = environment[key];
  if (value === undefined || value.length === 0 || value.trim() !== value) {
    throw new Error(`${key} must be a non-empty value without surrounding whitespace.`);
  }
  return value;
}

/**
 * @param environment - Environment variables to read.
 * @param key - Optional variable name.
 * @returns An optional non-empty value with no surrounding whitespace.
 */
function optional(environment: Environment, key: string): string | undefined {
  const value = environment[key];
  if (value === undefined || value.length === 0) return undefined;
  if (value.trim() !== value) {
    throw new Error(`${key} must not include surrounding whitespace.`);
  }
  return value;
}

/**
 * @param value - Stack name to validate.
 * @param key - Variable that supplied the stack name.
 * @returns The validated stack name.
 */
function stackName(value: string, key: string): string {
  if (!/^[A-Za-z][A-Za-z0-9-]{0,127}$/u.test(value)) {
    throw new Error(`${key} must be a CloudFormation stack name.`);
  }
  return value;
}

/**
 * @param value - Existing ACM certificate ARN to validate.
 * @returns A production-region ACM certificate ARN.
 */
function certificateArn(value: string): string {
  if (!/^arn:aws:acm:us-east-1:\d{12}:certificate\/[0-9a-f-]{36}$/u.test(value)) {
    throw new Error("CIPHER_ACM_CERTIFICATE_ARN must name a us-east-1 ACM certificate.");
  }
  return value;
}

/**
 * @param value - Existing Route 53 hosted-zone ID to validate.
 * @returns A Route 53 hosted-zone identifier without a path prefix.
 */
function hostedZoneId(value: string): string {
  if (!/^Z[A-Z0-9]{1,31}$/u.test(value)) {
    throw new Error("CIPHER_HOSTED_ZONE_ID must be a Route 53 hosted-zone ID.");
  }
  return value;
}

/**
 * @param value - Cost-alert recipient address to validate.
 * @returns A basic single-address value accepted by AWS Budgets.
 */
function budgetAlertEmail(value: string): string {
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(value)) {
    throw new Error("CIPHER_BUDGET_ALERT_EMAIL must be one email address.");
  }
  return value;
}

/**
 * @param value - Optional Secrets Manager ARN to validate.
 * @returns A complete same-region secret ARN, or undefined when no runtime secret is needed.
 */
function runtimeSecretArn(value: string | undefined): string | undefined {
  if (value === undefined) return undefined;
  if (!/^arn:aws:secretsmanager:us-east-1:\d{12}:secret:[A-Za-z0-9/_+=.@-]+$/u.test(value)) {
    throw new Error(
      "CIPHER_RUNTIME_SECRET_ARN must name a complete us-east-1 Secrets Manager secret ARN.",
    );
  }
  return value;
}

/**
 * @param environment - Environment variables to load.
 * @returns Validated infrastructure configuration.
 */
export function loadInfrastructureConfig(environment: Environment): InfrastructureConfig {
  const awsRegion = required(environment, "CIPHER_AWS_REGION");
  if (!/^[a-z]{2}(?:-gov)?-[a-z]+-\d+$/u.test(awsRegion)) {
    throw new Error("CIPHER_AWS_REGION must be an AWS region name.");
  }

  return {
    awsRegion,
    budgetAlertEmail: budgetAlertEmail(required(environment, "CIPHER_BUDGET_ALERT_EMAIL")),
    certificateArn: certificateArn(required(environment, "CIPHER_ACM_CERTIFICATE_ARN")),
    hostedZoneId: hostedZoneId(required(environment, "CIPHER_HOSTED_ZONE_ID")),
    runtimeSecretArn: runtimeSecretArn(optional(environment, "CIPHER_RUNTIME_SECRET_ARN")),
    stacks: {
      state: stackName(required(environment, "CIPHER_STATE_STACK"), "CIPHER_STATE_STACK"),
      control: stackName(required(environment, "CIPHER_CONTROL_STACK"), "CIPHER_CONTROL_STACK"),
      network: stackName(required(environment, "CIPHER_NETWORK_STACK"), "CIPHER_NETWORK_STACK"),
      runtime: stackName(required(environment, "CIPHER_RUNTIME_STACK"), "CIPHER_RUNTIME_STACK"),
    },
  };
}
