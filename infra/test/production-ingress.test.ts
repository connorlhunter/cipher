import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { productionIngress } from "../lib/production-ingress.js";

describe("Cipher production ingress", () => {
  test("keeps one TLS public entry point and the documented native endpoints", () => {
    assert.equal(productionIngress.region, "us-east-1");
    assert.deepEqual(productionIngress.listener, { port: 443, protocol: "HTTPS" });
    assert.deepEqual(productionIngress.dns, {
      hostname: "cipher.connorhunter.me",
      recordType: "A",
      zoneName: "connorhunter.me",
    });
    assert.deepEqual(productionIngress.certificate, {
      region: "us-east-1",
      source: "existing-wildcard",
    });
    assert.equal(productionIngress.endpoints.apiOrigin, "https://cipher.connorhunter.me");
    assert.equal(
      productionIngress.endpoints.realtimeUrl,
      "wss://cipher.connorhunter.me/v1/realtime",
    );
    assert.equal(productionIngress.endpoints.healthCheckPath, "/healthz");
  });

  test("keeps the single task reachable only through the load balancer shape", () => {
    assert.equal(productionIngress.availabilityZones, 2);
    assert.equal(productionIngress.natGateways, 0);
    assert.deepEqual(productionIngress.task, {
      assignPublicIp: true,
      port: 3000,
      protocol: "HTTP",
      subnetType: "public",
    });
    assert.deepEqual(productionIngress.websocket, {
      authorizationHeader: "Authorization",
      path: "/v1/realtime",
    });
  });
});
