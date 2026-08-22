/** Provisions the one DNS-validated certificate kept outside Cipher's four stack controls. */

const hostname = "cipher.connorhunter.me";
const zoneName = "connorhunter.me.";

/** Captured result from one AWS CLI invocation. */
export interface CommandResult {
  readonly exitCode: number;
  readonly stderr: string;
  readonly stdout: string;
}

/** Executes the narrow AWS CLI calls needed to provision the public certificate. */
export interface CommandRunner {
  run(command: readonly string[]): CommandResult;
}

/** Parsed production certificate configuration. */
export interface CertificateProvisionConfig {
  readonly accountId: string;
  readonly hostedZoneId?: string;
  readonly region: string;
}

interface CertificateSummary {
  readonly CertificateArn?: unknown;
  readonly DomainName?: unknown;
}

interface CertificateListResponse {
  readonly CertificateSummaryList?: readonly CertificateSummary[];
}

interface CertificateDetailsResponse {
  readonly Certificate?: {
    readonly DomainValidationOptions?: readonly {
      readonly DomainName?: unknown;
      readonly ResourceRecord?: {
        readonly Name?: unknown;
        readonly Type?: unknown;
        readonly Value?: unknown;
      };
    }[];
  };
}

interface HostedZoneResponse {
  readonly HostedZones?: readonly { readonly Id?: unknown; readonly Name?: unknown }[];
}

interface RecordSetResponse {
  readonly ResourceRecordSets?: readonly {
    readonly Name?: unknown;
    readonly ResourceRecords?: readonly { readonly Value?: unknown }[];
    readonly Type?: unknown;
  }[];
}

interface ParsedArguments {
  readonly confirmation?: string;
  readonly dryRun: boolean;
}

/** Executes commands with no shell interpolation. */
export const liveRunner: CommandRunner = {
  run(command) {
    const result = Bun.spawnSync([...command], { stderr: "pipe", stdout: "pipe" });
    return {
      exitCode: result.exitCode,
      stderr: new TextDecoder().decode(result.stderr),
      stdout: new TextDecoder().decode(result.stdout),
    };
  },
};

/**
 * Parses the narrow confirmation surface for certificate provisioning.
 *
 * @param args - Command-line arguments supplied after the package script.
 * @returns Validated confirmation and dry-run values.
 */
export function parseArguments(args: readonly string[]): ParsedArguments {
  let confirmation: string | undefined;
  let dryRun = false;
  for (const value of args) {
    if (value === "--dry-run") {
      dryRun = true;
      continue;
    }
    if (value.startsWith("--confirm=")) {
      confirmation = value.slice("--confirm=".length);
      continue;
    }
    throw new Error(`Unknown certificate provisioning option: ${value}`);
  }
  return { confirmation, dryRun };
}

/**
 * @param environment - Process environment to validate.
 * @returns Production account and optional known hosted-zone configuration.
 */
export function loadCertificateProvisionConfig(
  environment: Readonly<Record<string, string | undefined>>,
): CertificateProvisionConfig {
  const accountId = required(environment, "CIPHER_AWS_ACCOUNT_ID");
  if (!/^\d{12}$/u.test(accountId) || accountId === "000000000000") {
    throw new Error("CIPHER_AWS_ACCOUNT_ID must name the 12-digit production account.");
  }
  const region = required(environment, "CIPHER_AWS_REGION");
  if (region !== "us-east-1") {
    throw new Error("CIPHER_AWS_REGION must be us-east-1 for the public certificate.");
  }
  const hostedZoneId = optional(environment, "CIPHER_HOSTED_ZONE_ID");
  if (hostedZoneId !== undefined && !/^Z[A-Z0-9]{1,31}$/u.test(hostedZoneId)) {
    throw new Error("CIPHER_HOSTED_ZONE_ID must be a Route 53 hosted-zone ID.");
  }
  return { accountId, hostedZoneId, region };
}

/**
 * Resolves or creates the exact certificate and DNS validation record.
 *
 * @param args - Command-line confirmation values.
 * @param environment - Production environment configuration.
 * @param runner - AWS CLI executor.
 * @returns Display-ready next steps without leaking credentials.
 */
export function provisionProductionCertificate(
  args: readonly string[],
  environment: Readonly<Record<string, string | undefined>>,
  runner: CommandRunner = liveRunner,
): string[] {
  const config = loadCertificateProvisionConfig(environment);
  const { confirmation, dryRun } = parseArguments(args);
  assertActiveAccount(config, runner);
  const hostedZoneId = config.hostedZoneId ?? findHostedZone(config, runner);
  const certificate = findCertificate(config, runner);
  if (certificate?.status === "issued") {
    return configuredCertificateMessages(certificate.arn, hostedZoneId);
  }

  const expectedConfirmation = `PROVISION-CIPHER-PRODUCTION-CERTIFICATE-${config.accountId}-${config.region}`;
  if (confirmation !== expectedConfirmation) {
    throw new Error(`Refusing certificate provisioning: pass --confirm=${expectedConfirmation}.`);
  }

  if (dryRun) {
    return [
      `Would ${certificate === undefined ? "request" : "reuse"} the ${hostname} ACM certificate.`,
      `Would upsert its DNS validation record in ${zoneName} (${hostedZoneId}).`,
    ];
  }

  const certificateArn = certificate?.arn ?? requestCertificate(config, runner);
  const record = validationRecord(certificateArn, config, runner);
  upsertValidationRecord(record, hostedZoneId, config, runner);
  return [
    "DNS validation is in place; ACM issues the certificate after its validation check completes.",
    ...configuredCertificateMessages(certificateArn, hostedZoneId),
  ];
}

/**
 * @param environment - Environment variables to inspect.
 * @param key - Required key.
 * @returns A non-empty value with no surrounding whitespace.
 */
function required(environment: Readonly<Record<string, string | undefined>>, key: string): string {
  const value = environment[key];
  if (value === undefined || value.length === 0 || value.trim() !== value) {
    throw new Error(`${key} must be a non-empty value without surrounding whitespace.`);
  }
  return value;
}

/**
 * @param environment - Environment variables to inspect.
 * @param key - Optional key.
 * @returns An optional value with no surrounding whitespace.
 */
function optional(
  environment: Readonly<Record<string, string | undefined>>,
  key: string,
): string | undefined {
  const value = environment[key];
  if (value === undefined || value.length === 0) return undefined;
  if (value.trim() !== value) throw new Error(`${key} must not include surrounding whitespace.`);
  return value;
}

/**
 * @param config - Production certificate configuration.
 * @param runner - AWS CLI executor.
 * @returns Nothing; throws before changes when the active account differs.
 */
function assertActiveAccount(config: CertificateProvisionConfig, runner: CommandRunner): void {
  const account = run(
    [
      "aws",
      "sts",
      "get-caller-identity",
      "--query",
      "Account",
      "--output",
      "text",
      "--region",
      config.region,
    ],
    runner,
    "Could not verify the active AWS account.",
  ).trim();
  if (account !== config.accountId) {
    throw new Error(
      "The active AWS account is not Cipher production. No certificate changes were made.",
    );
  }
}

/**
 * @param config - Production certificate configuration.
 * @param runner - AWS CLI executor.
 * @returns The only exact matching certificate and its issuance state, if it exists.
 */
function findCertificate(
  config: CertificateProvisionConfig,
  runner: CommandRunner,
): { readonly arn: string; readonly status: "issued" | "pending" } | undefined {
  const issued = matchingCertificates(
    parseJson<CertificateListResponse>(
      run(
        aws(config, "acm", "list-certificates", "--certificate-statuses", "ISSUED"),
        runner,
        "Could not list issued ACM certificates.",
      ),
      "issued ACM certificates",
    ),
  );
  const pending = matchingCertificates(
    parseJson<CertificateListResponse>(
      run(
        aws(config, "acm", "list-certificates", "--certificate-statuses", "PENDING_VALIDATION"),
        runner,
        "Could not list pending ACM certificates.",
      ),
      "pending ACM certificates",
    ),
  );
  const matches = [...issued, ...pending];
  if (matches.length > 1) {
    throw new Error(
      `Found multiple ${hostname} ACM certificates; resolve the ambiguity before continuing.`,
    );
  }
  const match = matches[0];
  if (match === undefined) return undefined;
  return { arn: match.arn, status: issued.length === 1 ? "issued" : "pending" };
}

/**
 * @param response - ACM list response.
 * @returns Exact host certificate ARNs only.
 */
function matchingCertificates(
  response: CertificateListResponse,
): readonly { readonly arn: string }[] {
  return (response.CertificateSummaryList ?? []).flatMap((summary) => {
    if (summary.DomainName !== hostname || typeof summary.CertificateArn !== "string") return [];
    return [{ arn: summary.CertificateArn }];
  });
}

/**
 * @param config - Production certificate configuration.
 * @param runner - AWS CLI executor.
 * @returns Exact hosted-zone ID for the public hostname.
 */
function findHostedZone(config: CertificateProvisionConfig, runner: CommandRunner): string {
  const response = parseJson<HostedZoneResponse>(
    run(
      aws(config, "route53", "list-hosted-zones-by-name", "--dns-name", zoneName),
      runner,
      "Could not list the Route 53 hosted zone.",
    ),
    "the Route 53 hosted zone",
  );
  const zones = (response.HostedZones ?? []).flatMap((zone) => {
    if (zone.Name !== zoneName || typeof zone.Id !== "string") return [];
    const id = zone.Id.replace(/^\/hostedzone\//u, "");
    return /^Z[A-Z0-9]{1,31}$/u.test(id) ? [id] : [];
  });
  if (zones.length !== 1) {
    throw new Error(
      `Expected exactly one ${zoneName} hosted zone before certificate provisioning.`,
    );
  }
  return zones[0] as string;
}

/**
 * @param config - Production certificate configuration.
 * @param runner - AWS CLI executor.
 * @returns ACM ARN for the newly requested certificate.
 */
function requestCertificate(config: CertificateProvisionConfig, runner: CommandRunner): string {
  const arn = run(
    aws(
      config,
      "acm",
      "request-certificate",
      "--domain-name",
      hostname,
      "--validation-method",
      "DNS",
      "--idempotency-token",
      "cipherproduction",
      "--query",
      "CertificateArn",
      "--output",
      "text",
    ),
    runner,
    "Could not request the ACM certificate.",
  ).trim();
  if (!/^arn:aws:acm:us-east-1:\d{12}:certificate\/[0-9a-f-]{36}$/u.test(arn)) {
    throw new Error("ACM did not return a valid us-east-1 certificate ARN.");
  }
  return arn;
}

/**
 * @param certificateArn - ACM certificate whose DNS record is required.
 * @param config - Production certificate configuration.
 * @param runner - AWS CLI executor.
 * @returns Validated CNAME record required by ACM.
 */
function validationRecord(
  certificateArn: string,
  config: CertificateProvisionConfig,
  runner: CommandRunner,
): { readonly name: string; readonly value: string } {
  const response = parseJson<CertificateDetailsResponse>(
    run(
      aws(config, "acm", "describe-certificate", "--certificate-arn", certificateArn),
      runner,
      "Could not read the ACM validation record.",
    ),
    "the ACM validation record",
  );
  const option = response.Certificate?.DomainValidationOptions?.find(
    (candidate) => candidate.DomainName === hostname,
  );
  const name = option?.ResourceRecord?.Name;
  const type = option?.ResourceRecord?.Type;
  const value = option?.ResourceRecord?.Value;
  if (
    typeof name !== "string" ||
    !name.endsWith(`.${zoneName}`) ||
    type !== "CNAME" ||
    typeof value !== "string" ||
    !value.endsWith(".acm-validations.aws.")
  ) {
    throw new Error(
      "ACM has not published a valid DNS validation record yet. Rerun this command shortly.",
    );
  }
  return { name, value };
}

/**
 * @param record - Validated ACM CNAME record.
 * @param hostedZoneId - Exact Route 53 hosted-zone ID.
 * @param config - Production certificate configuration.
 * @param runner - AWS CLI executor.
 * @returns Nothing; refuses to replace a differing existing record.
 */
function upsertValidationRecord(
  record: { readonly name: string; readonly value: string },
  hostedZoneId: string,
  config: CertificateProvisionConfig,
  runner: CommandRunner,
): void {
  const response = parseJson<RecordSetResponse>(
    run(
      aws(
        config,
        "route53",
        "list-resource-record-sets",
        "--hosted-zone-id",
        hostedZoneId,
        "--start-record-name",
        record.name,
        "--start-record-type",
        "CNAME",
        "--max-items",
        "1",
      ),
      runner,
      "Could not inspect the ACM DNS validation record.",
    ),
    "the ACM DNS validation record",
  );
  const existing = response.ResourceRecordSets?.[0];
  if (existing?.Name === record.name && existing.Type === "CNAME") {
    const values = existing.ResourceRecords?.map((value) => value.Value);
    if (values?.length === 1 && values[0] === record.value) return;
    throw new Error("The ACM DNS validation name already has a different record value.");
  }

  const changeBatch = JSON.stringify({
    Changes: [
      {
        Action: "UPSERT",
        ResourceRecordSet: {
          Name: record.name,
          ResourceRecords: [{ Value: record.value }],
          TTL: 300,
          Type: "CNAME",
        },
      },
    ],
  });
  run(
    aws(
      config,
      "route53",
      "change-resource-record-sets",
      "--hosted-zone-id",
      hostedZoneId,
      "--change-batch",
      changeBatch,
    ),
    runner,
    "Could not create the ACM DNS validation record.",
  );
}

/**
 * @param config - Production certificate configuration.
 * @param service - AWS CLI service name.
 * @param args - Service-specific arguments.
 * @returns Safe direct AWS CLI argument list.
 */
function aws(config: CertificateProvisionConfig, service: string, ...args: string[]): string[] {
  return ["aws", service, ...args, "--region", config.region];
}

/**
 * @param command - Command to execute.
 * @param runner - AWS CLI executor.
 * @param failure - User-actionable failure prefix.
 * @returns Captured standard output on success.
 */
function run(command: readonly string[], runner: CommandRunner, failure: string): string {
  const result = runner.run(command);
  if (result.exitCode !== 0) {
    throw new Error(`${failure} ${result.stderr.trim() || "AWS CLI exited unsuccessfully."}`);
  }
  return result.stdout;
}

/**
 * @param value - JSON text from AWS CLI.
 * @param subject - Human-readable subject for error messages.
 * @returns Parsed JSON payload.
 */
function parseJson<T>(value: string, subject: string): T {
  try {
    return JSON.parse(value) as T;
  } catch {
    throw new Error(`Could not read ${subject}.`);
  }
}

/**
 * @param certificateArn - Issued or pending certificate ARN.
 * @param hostedZoneId - Exact hosted-zone ID used for validation and ingress DNS.
 * @returns Configuration lines to persist in the production environment.
 */
function configuredCertificateMessages(certificateArn: string, hostedZoneId: string): string[] {
  return [`CIPHER_ACM_CERTIFICATE_ARN=${certificateArn}`, `CIPHER_HOSTED_ZONE_ID=${hostedZoneId}`];
}

if (import.meta.main) {
  try {
    for (const message of provisionProductionCertificate(process.argv.slice(2), process.env)) {
      console.log(message);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
