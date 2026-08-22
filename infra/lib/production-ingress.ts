/**
 * Fixed network and public-endpoint choices for Cipher's one production environment.
 *
 * The network and runtime stacks consume this contract so they cannot independently
 * choose a hostname, listener, task port, or NAT topology.
 */
export const productionIngress = {
  availabilityZones: 2,
  certificate: {
    region: "us-east-1",
    source: "existing-wildcard",
  },
  dns: {
    hostname: "cipher.connorhunter.me",
    recordType: "A",
    zoneName: "connorhunter.me",
  },
  endpoints: {
    apiOrigin: "https://cipher.connorhunter.me",
    healthCheckPath: "/healthz",
    realtimeUrl: "wss://cipher.connorhunter.me/v1/realtime",
  },
  listener: {
    port: 443,
    protocol: "HTTPS",
  },
  natGateways: 0,
  region: "us-east-1",
  task: {
    assignPublicIp: true,
    port: 3000,
    protocol: "HTTP",
    subnetType: "public",
  },
  websocket: {
    authorizationHeader: "Authorization",
    path: "/v1/realtime",
  },
} as const;
