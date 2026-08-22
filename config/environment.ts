/** Environment variables used by deployment and infrastructure controls. */
export type Environment = Readonly<Record<string, string | undefined>>;

/** Exact CloudFormation stack names for one Cipher deployment. */
export interface InfrastructureConfig {
  awsRegion: string;
  certificateArn: string;
  hostedZoneId: string;
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
    certificateArn: certificateArn(required(environment, "CIPHER_ACM_CERTIFICATE_ARN")),
    hostedZoneId: hostedZoneId(required(environment, "CIPHER_HOSTED_ZONE_ID")),
    stacks: {
      state: stackName(required(environment, "CIPHER_STATE_STACK"), "CIPHER_STATE_STACK"),
      control: stackName(required(environment, "CIPHER_CONTROL_STACK"), "CIPHER_CONTROL_STACK"),
      network: stackName(required(environment, "CIPHER_NETWORK_STACK"), "CIPHER_NETWORK_STACK"),
      runtime: stackName(required(environment, "CIPHER_RUNTIME_STACK"), "CIPHER_RUNTIME_STACK"),
    },
  };
}
