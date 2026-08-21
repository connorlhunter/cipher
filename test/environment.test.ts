import { describe, expect, test } from "bun:test";

import { loadInfrastructureConfig } from "../config/environment";

const validEnvironment = {
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_CONTROL_STACK: "CipherProductionControl",
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
      loadInfrastructureConfig({ ...validEnvironment, CIPHER_AWS_REGION: "east" }),
    ).toThrow("CIPHER_AWS_REGION must be an AWS region name");
  });
});
