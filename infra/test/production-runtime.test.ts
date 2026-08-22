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
const controlSettings = {
  budgetAlertEmail: "production-alerts@example.invalid",
  stackNames: {
    control: "Control",
    network: "Network",
    runtime: "Runtime",
    state: "State",
  },
};

function templates(
  runtimeSecretArn?: string,
  allowDestruction = false,
): { control: Template; runtime: Template } {
  const app = new cdk.App();
  configureProductionNetworkContext(app, account, region);
  const environment = { account, region };
  const stateStack = new cdk.Stack(app, "State", { env: environment });
  const controlStack = new cdk.Stack(app, "Control", { env: environment });
  const networkStack = new cdk.Stack(app, "Network", { env: environment });
  const runtimeStack = new cdk.Stack(app, "Runtime", { env: environment });

  const state = addStateFoundations(stateStack, { allowDestruction });
  const control = addProductionControl(controlStack, state, {
    ...controlSettings,
    allowDestruction,
  });
  const network = addProductionNetwork(networkStack);
  addProductionRuntime(runtimeStack, network, state, control, {
    ...runtimeSettings,
    runtimeSecretArn,
  });

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

function roleByName(template: Template, roleName: string): CloudFormationResource {
  const match = resources(template, "AWS::IAM::Role").find(
    (resource) => properties(resource).RoleName === roleName,
  );
  assert.ok(match, `expected ${roleName}`);
  return match;
}

function tags(resource: CloudFormationResource): Record<string, string> {
  const declaredTags = properties(resource).Tags;
  assert.ok(Array.isArray(declaredTags), "expected resource tags");
  return Object.fromEntries(
    declaredTags.map((tag) => {
      assert.ok(typeof tag === "object" && tag !== null, "expected tag object");
      const value = tag as { Key?: unknown; Value?: unknown };
      assert.equal(typeof value.Key, "string", "expected tag key");
      assert.equal(typeof value.Value, "string", "expected tag value");
      return [value.Key, value.Value];
    }),
  );
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

  test("switches retained operations resources to destructive mode only when requested", () => {
    const template = templates(undefined, true).control;
    const repository = onlyResource(template, "AWS::ECR::Repository");
    const logGroup = onlyResource(template, "AWS::Logs::LogGroup");
    const backupVault = onlyResource(template, "AWS::Backup::BackupVault");

    for (const resource of [repository, logGroup, backupVault]) {
      assert.equal(resource.DeletionPolicy, "Delete");
      assert.equal(resource.UpdateReplacePolicy, "Delete");
    }
    assert.equal(properties(repository).EmptyOnDelete, true);
  });

  test("limits deployment identity, runtime roles, backup retention, and cost controls", () => {
    const template = templates().control;
    const document = template.toJSON() as { readonly Outputs?: Record<string, unknown> };
    const provider = onlyResource(template, "AWS::IAM::OIDCProvider");
    const deploymentRole = roleByName(template, "cipher-production-deployment");
    const taskRole = roleByName(template, "cipher-production-task");
    const executionRole = roleByName(template, "cipher-production-execution");
    const backupRole = roleByName(template, "cipher-production-AWSBackup");
    const backupVault = onlyResource(template, "AWS::Backup::BackupVault");
    const backupPlan = onlyResource(template, "AWS::Backup::BackupPlan");
    const backupSelection = onlyResource(template, "AWS::Backup::BackupSelection");
    const budget = onlyResource(template, "AWS::Budgets::Budget");
    const trust = properties(deploymentRole).AssumeRolePolicyDocument as {
      Statement?: readonly { readonly Condition?: unknown; readonly Principal?: unknown }[];
    };
    const deploymentDocument = JSON.stringify(template.toJSON());

    assert.deepEqual(properties(provider).ClientIdList, ["sts.amazonaws.com"]);
    assert.equal(properties(provider).Url, "https://token.actions.githubusercontent.com");
    assert.deepEqual(trust.Statement?.[0]?.Condition, {
      StringEquals: {
        "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
        "token.actions.githubusercontent.com:sub":
          "repo:connorlhunter/cipher:environment:production",
      },
    });
    assert.equal(properties(deploymentRole).MaxSessionDuration, 3600);
    assert.deepEqual(tags(deploymentRole), {
      Application: "cipher",
      CostCenter: "cipher-production",
      Environment: "production",
      ManagedBy: "cdk",
    });
    assert.equal("ManagedPolicyArns" in properties(taskRole), false);
    assert.equal("Policies" in properties(taskRole), false);
    assert.equal(properties(executionRole).RoleName, "cipher-production-execution");
    assert.match(
      JSON.stringify(properties(backupRole).ManagedPolicyArns),
      /AWSBackupServiceRolePolicyForBackup/u,
    );
    assert.equal(backupVault.DeletionPolicy, "Retain");
    assert.equal(backupVault.UpdateReplacePolicy, "Retain");
    assert.equal(properties(backupVault).BackupVaultName, "cipher-production-recovery");
    assert.equal("EncryptionKeyArn" in properties(backupVault), false);
    assert.ok(properties(backupPlan).BackupPlan, "expected a 35-day AWS Backup plan");
    const backupSelectionProperties = properties(backupSelection).BackupSelection as {
      readonly Resources?: unknown;
    };
    assert.ok(Array.isArray(backupSelectionProperties.Resources));
    assert.equal(backupSelectionProperties.Resources.length, 4);
    assert.deepEqual(properties(budget).Budget, {
      BudgetLimit: { Amount: 50, Unit: "USD" },
      BudgetName: "cipher-production-monthly",
      BudgetType: "COST",
      CostFilters: { TagKeyValue: ["user:Application$cipher"] },
      TimeUnit: "MONTHLY",
    });
    assert.equal((properties(budget).NotificationsWithSubscribers as unknown[]).length, 2);
    assert.doesNotMatch(deploymentDocument, /AdministratorAccess/u);
    assert.doesNotMatch(deploymentDocument, /:iam:us-east-1:/u);
    assert.match(deploymentDocument, /cloudformation:DescribeStacks/u);
    for (const stack of ["CDKToolkit", "State", "Control", "Network", "Runtime"]) {
      assert.match(deploymentDocument, new RegExp(`stack/${stack}/\\*`, "u"));
    }
    assert.match(deploymentDocument, /ecs:DescribeServices/u);
    assert.match(deploymentDocument, /service\/cipher-production\/cipher-production-server/u);
    assert.match(deploymentDocument, /backup:StartBackupJob/u);
    assert.match(
      deploymentDocument,
      /"Action":"backup:StartBackupJob","Effect":"Allow","Resource":"\*"/u,
    );
    assert.match(deploymentDocument, /\/fixtures\/\*/u);
    for (const output of [
      "ServerRepositoryName",
      "ServerRepositoryUri",
      "DeploymentRoleArn",
      "BackupRoleArn",
      "BackupVaultName",
    ]) {
      assert.ok(document.Outputs?.[output], `expected ${output}`);
    }
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
    const { control, runtime } = templates();
    const template = runtime;
    const alias = properties(onlyResource(template, "AWS::Route53::RecordSet"));
    const logGroup = onlyResource(control, "AWS::Logs::LogGroup");
    const document = template.toJSON() as {
      readonly Parameters?: Record<string, Record<string, unknown>>;
    };

    assert.equal(alias.Name, "cipher.connorhunter.me.");
    assert.equal(alias.Type, "A");
    assert.equal(alias.HostedZoneId, runtimeSettings.hostedZoneId);
    assert.ok(alias.AliasTarget, "expected load balancer alias target");
    assert.equal(logGroup.DeletionPolicy, "Retain");
    assert.equal(logGroup.UpdateReplacePolicy, "Retain");
    assert.equal(properties(logGroup).LogGroupName, "/cipher/production/server");
    assert.equal(properties(logGroup).RetentionInDays, 30);
    assert.deepEqual(document.Parameters?.ServerImageTag, {
      AllowedPattern: "[A-Za-z0-9][A-Za-z0-9._-]{0,127}",
      Default: "bootstrap",
      Description: "Immutable ECR tag for the Cipher server image.",
      MinLength: 1,
      Type: "String",
    });
  });

  test("delivers an optional runtime secret only through the execution role", () => {
    const runtimeSecretArn =
      "arn:aws:secretsmanager:us-east-1:123456789012:secret:cipher-production-runtime-AbCdEf";
    const { control, runtime } = templates(runtimeSecretArn);
    const taskDefinition = properties(onlyResource(runtime, "AWS::ECS::TaskDefinition"));
    const container = (
      taskDefinition.ContainerDefinitions as readonly { readonly Secrets?: unknown }[]
    )[0];

    assert.ok(container, "expected one server container");
    assert.deepEqual(container.Secrets, [
      { Name: "CIPHER_RUNTIME_SECRET", ValueFrom: runtimeSecretArn },
    ]);
    assert.match(JSON.stringify(control.toJSON()), /secretsmanager:GetSecretValue/u);
  });
});
