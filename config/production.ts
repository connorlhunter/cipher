import values from "./production.json" with { type: "json" };

/**
 * Public, non-secret settings that define Cipher's production boundary.
 *
 * @property awsRegion - AWS region containing the production deployment.
 * @property apiOrigin - Public HTTPS API origin.
 * @property realtimeUrl - Public secure WebSocket endpoint.
 * @property stacks - Exact CloudFormation stack names.
 */
export interface ProductionConfig {
  awsRegion: string;
  apiOrigin: string;
  realtimeUrl: string;
  stacks: {
    state: string;
    control: string;
    network: string;
    runtime: string;
  };
}

/** @description Canonical public settings shared by deployment scripts and the backend. */
export const productionConfig: Readonly<ProductionConfig> = values;
