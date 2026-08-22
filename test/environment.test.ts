import { describe, expect, test } from "bun:test";

import { loadInfrastructureConfig } from "../config/environment";

const validEnvironment = {
  CIPHER_ACM_CERTIFICATE_ARN:
    "arn:aws:acm:us-east-1:123456789012:certificate/00000000-0000-4000-8000-000000000000",
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_CONTROL_STACK: "CipherProductionControl",
  CIPHER_HOSTED_ZONE_ID: "Z000000000000000000000",
  CIPHER_NETWORK_STACK: "CipherProductionNetwork",
  CIPHER_RUNTIME_STACK: "CipherProductionRuntime",
  CIPHER_STATE_STACK: "CipherProductionState",
};

describe("infrastructure environment", () => {
  test("rejects missing, malformed, and non-region values", () => {
    expect(() => loadInfrastructureConfig({ ...validEnvironment, CIPHER_STATE_STACK: "" })).toThrow(
      "CIPHER_STATE_STACK must be a non-empty value",
    );
    expect(() =>
      loadInfrastructureConfig({ ...validEnvironment, CIPHER_CONTROL_STACK: "not a stack" }),
    ).toThrow("CIPHER_CONTROL_STACK must be a CloudFormation stack name");
    expect(() =>
      loadInfrastructureConfig({ ...validEnvironment, CIPHER_ACM_CERTIFICATE_ARN: "invalid" }),
    ).toThrow("CIPHER_ACM_CERTIFICATE_ARN must name a us-east-1 ACM certificate");
    expect(() =>
      loadInfrastructureConfig({ ...validEnvironment, CIPHER_HOSTED_ZONE_ID: "zone" }),
    ).toThrow("CIPHER_HOSTED_ZONE_ID must be a Route 53 hosted-zone ID");
    expect(() =>
      loadInfrastructureConfig({ ...validEnvironment, CIPHER_AWS_REGION: "east" }),
    ).toThrow("CIPHER_AWS_REGION must be an AWS region name");
  });
});
