import assert from "node:assert/strict";
import { describe, test } from "node:test";

import * as cdk from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";

import { addProductionControl } from "../lib/production-control.js";
import {
  addProductionNetwork,
  configureProductionNetworkContext,
} from "../lib/production-network.js";
import { addProductionRuntime } from "../lib/production-runtime.js";
import { addStateFoundations } from "../lib/state-foundations.js";

interface CloudFormationResource {
  readonly DeletionPolicy?: unknown;
  readonly Properties?: Record<string, unknown>;
  readonly UpdateReplacePolicy?: unknown;
}

const account = "123456789012";
const region = "us-east-1";
const runtimeSettings = {
  certificateArn: `arn:aws:acm:${region}:${account}:certificate/00000000-0000-4000-8000-000000000000`,
  hostedZoneId: "Z000000000000000000000",
};

function templates(): { control: Template; runtime: Template } {
  const app = new cdk.App();
  configureProductionNetworkContext(app, account, region);
  const environment = { account, region };
  const stateStack = new cdk.Stack(app, "State", { env: environment });
  const controlStack = new cdk.Stack(app, "Control", { env: environment });
  const networkStack = new cdk.Stack(app, "Network", { env: environment });
  const runtimeStack = new cdk.Stack(app, "Runtime", { env: environment });

  const state = addStateFoundations(stateStack);
  const control = addProductionControl(controlStack);
  const network = addProductionNetwork(networkStack);
  addProductionRuntime(runtimeStack, network, state, control, runtimeSettings);

  return { control: Template.fromStack(controlStack), runtime: Template.fromStack(runtimeStack) };
}

function resources(template: Template, type: string): CloudFormationResource[] {
  return Object.values(template.findResources(type)) as CloudFormationResource[];
}

function onlyResource(template: Template, type: string): CloudFormationResource {
  const matches = resources(template, type);
  assert.equal(matches.length, 1, `expected one ${type} resource`);
  return matches[0] as CloudFormationResource;
}

function properties(resource: CloudFormationResource): Record<string, unknown> {
  assert.ok(resource.Properties, "expected CloudFormation resource properties");
  return resource.Properties;
}

function attributes(values: unknown): Record<string, string> {
  assert.ok(Array.isArray(values), "expected load balancer attributes");
  return Object.fromEntries(
    values.map((value) => {
      assert.ok(typeof value === "object" && value !== null, "expected attribute object");
      const attribute = value as { Key?: unknown; Value?: unknown };
      assert.equal(typeof attribute.Key, "string", "expected attribute key");
      assert.equal(typeof attribute.Value, "string", "expected attribute value");
      return [attribute.Key, attribute.Value];
    }),
  );
}

function containerEnvironment(taskDefinition: CloudFormationResource): Record<string, unknown> {
  const containers = properties(taskDefinition).ContainerDefinitions;
  assert.ok(Array.isArray(containers), "expected one container definition");
  assert.equal(containers.length, 1);
  const container = containers[0] as { Environment?: unknown };
  assert.ok(Array.isArray(container.Environment), "expected container environment");
  return Object.fromEntries(
    container.Environment.map((entry) => {
      assert.ok(typeof entry === "object" && entry !== null, "expected environment entry");
      const value = entry as { Name?: unknown; Value?: unknown };
      assert.equal(typeof value.Name, "string", "expected environment name");
      return [value.Name, value.Value];
    }),
  );
}

describe("Cipher production control and runtime", () => {
  test("keeps a dedicated immutable scanned server repository with bounded retention", () => {
    const repository = onlyResource(templates().control, "AWS::ECR::Repository");
    const repositoryProperties = properties(repository);

    assert.equal(repository.DeletionPolicy, "Retain");
    assert.equal(repository.UpdateReplacePolicy, "Retain");
    assert.equal(repositoryProperties.RepositoryName, "cipher-production-server");
    assert.equal(repositoryProperties.ImageTagMutability, "IMMUTABLE");
    assert.deepEqual(repositoryProperties.ImageScanningConfiguration, { ScanOnPush: true });
    assert.equal(typeof repositoryProperties.LifecyclePolicy, "object");
    const lifecycle = repositoryProperties.LifecyclePolicy as { LifecyclePolicyText?: unknown };
    assert.equal(typeof lifecycle.LifecyclePolicyText, "string");
    assert.match(lifecycle.LifecyclePolicyText as string, /"countNumber":20/u);
  });

  test("runs exactly one stop-before-start backend task behind TLS ingress", () => {
    const template = templates().runtime;
    const service = properties(onlyResource(template, "AWS::ECS::Service"));
    const taskDefinition = onlyResource(template, "AWS::ECS::TaskDefinition");
    const taskProperties = properties(taskDefinition);
    const loadBalancer = properties(
      onlyResource(template, "AWS::ElasticLoadBalancingV2::LoadBalancer"),
    );
    const listener = properties(onlyResource(template, "AWS::ElasticLoadBalancingV2::Listener"));
    const targetGroup = properties(
      onlyResource(template, "AWS::ElasticLoadBalancingV2::TargetGroup"),
    );

    assert.equal(resources(template, "AWS::ECS::Service").length, 1);
    assert.equal(service.DesiredCount, 1);
    assert.equal(service.LaunchType, "FARGATE");
    assert.deepEqual(service.DeploymentController, { Type: "ECS" });
    assert.deepEqual(service.DeploymentConfiguration, {
      Alarms: { AlarmNames: [], Enable: false, Rollback: false },
      DeploymentCircuitBreaker: { Enable: true, Rollback: true },
      MaximumPercent: 100,
      MinimumHealthyPercent: 0,
    });
    assert.equal(service.AvailabilityZoneRebalancing, "DISABLED");
    assert.equal(service.HealthCheckGracePeriodSeconds, 60);

    assert.equal(taskProperties.Cpu, "256");
    assert.equal(taskProperties.Memory, "512");
    assert.equal(taskProperties.Family, "cipher-production-server");
    assert.deepEqual(taskProperties.RequiresCompatibilities, ["FARGATE"]);
    const environment = containerEnvironment(taskDefinition);
    assert.equal(environment.CIPHER_SERVER_BIND, "0.0.0.0:3000");
    assert.equal(environment.CIPHER_API_ORIGIN, "https://cipher.connorhunter.me");
    assert.equal(environment.CIPHER_REALTIME_URL, "wss://cipher.connorhunter.me/v1/realtime");
    assert.deepEqual(environment.CIPHER_AWS_ACCOUNT_ID, { Ref: "AWS::AccountId" });
    assert.deepEqual(environment.CIPHER_AWS_REGION, { Ref: "AWS::Region" });
    for (const name of [
      "CIPHER_AWS_ACCOUNT_ID",
      "CIPHER_AWS_REGION",
      "CIPHER_COGNITO_CLIENT_ID",
      "CIPHER_COGNITO_USER_POOL_ID",
      "CIPHER_CONVERSATIONS_TABLE",
      "CIPHER_MEDIA_BUCKET",
      "CIPHER_MEDIA_TABLE",
      "CIPHER_MESSAGES_TABLE",
      "CIPHER_USERS_TABLE",
      "RUST_LOG",
    ]) {
      assert.ok(environment[name], `expected ${name}`);
    }
    assert.match(JSON.stringify(taskProperties.ContainerDefinitions), /"ServerImageTag"/u);
    assert.doesNotMatch(JSON.stringify(taskProperties.ContainerDefinitions), /latest/u);

    assert.equal(loadBalancer.Scheme, "internet-facing");
    assert.equal(loadBalancer.Type, "application");
    assert.deepEqual(attributes(loadBalancer.LoadBalancerAttributes), {
      "deletion_protection.enabled": "false",
      "idle_timeout.timeout_seconds": "300",
      "routing.http.drop_invalid_header_fields.enabled": "true",
      "routing.http2.enabled": "true",
    });
    assert.equal(listener.Port, 443);
    assert.equal(listener.Protocol, "HTTPS");
    assert.deepEqual(listener.Certificates, [{ CertificateArn: runtimeSettings.certificateArn }]);
    assert.equal(resources(template, "AWS::ElasticLoadBalancingV2::Listener").length, 1);
    assert.equal(targetGroup.HealthCheckPath, "/healthz");
    assert.deepEqual(targetGroup.Matcher, { HttpCode: "200" });
    assert.equal(targetGroup.Protocol, "HTTP");
    assert.equal(targetGroup.ProtocolVersion, "HTTP1");
    assert.deepEqual(attributes(targetGroup.TargetGroupAttributes), {
      "deregistration_delay.timeout_seconds": "60",
      "stickiness.enabled": "false",
    });
  });

  test("publishes one A alias and retains bounded production logs", () => {
    const template = templates().runtime;
    const alias = properties(onlyResource(template, "AWS::Route53::RecordSet"));
    const logGroup = onlyResource(template, "AWS::Logs::LogGroup");
    const document = template.toJSON() as {
      readonly Parameters?: Record<string, Record<string, unknown>>;
    };

    assert.equal(alias.Name, "cipher.connorhunter.me.");
    assert.equal(alias.Type, "A");
    assert.equal(alias.HostedZoneId, runtimeSettings.hostedZoneId);
    assert.ok(alias.AliasTarget, "expected load balancer alias target");
    assert.equal(logGroup.DeletionPolicy, "Retain");
    assert.equal(logGroup.UpdateReplacePolicy, "Retain");
    assert.deepEqual(properties(logGroup), {
      LogGroupName: "/cipher/production/server",
      RetentionInDays: 30,
    });
    assert.deepEqual(document.Parameters?.ServerImageTag, {
      AllowedPattern: "[A-Za-z0-9][A-Za-z0-9._-]{0,127}",
      Default: "bootstrap",
      Description: "Immutable ECR tag for the Cipher server image.",
      MinLength: 1,
      Type: "String",
    });
  });
});
