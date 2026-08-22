/** Defines Cipher's one production backend service, TLS ingress, and DNS alias. */
import * as cdk from "aws-cdk-lib";
import * as acm from "aws-cdk-lib/aws-certificatemanager";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import * as ecs from "aws-cdk-lib/aws-ecs";
import * as elbv2 from "aws-cdk-lib/aws-elasticloadbalancingv2";
import * as route53 from "aws-cdk-lib/aws-route53";
import * as route53Targets from "aws-cdk-lib/aws-route53-targets";
import * as secretsmanager from "aws-cdk-lib/aws-secretsmanager";

import type { ProductionControl } from "./production-control.js";
import { productionIngress } from "./production-ingress.js";
import { addProductionTags, type ProductionNetwork } from "./production-network.js";
import type { StateFoundations } from "./state-foundations.js";

const serverContainerName = "cipher-server";
const serverImageTagDefault = "bootstrap";
const serverServiceName = "cipher-production-server";
const serverTaskFamily = "cipher-production-server";

/** Values that identify the existing DNS and TLS resources owned outside Cipher stacks. */
export interface ProductionRuntimeSettings {
  /** Existing wildcard certificate in the production region. */
  readonly certificateArn: string;
  /** Existing Route 53 hosted-zone ID for Cipher's public hostname. */
  readonly hostedZoneId: string;
  /** Optional Secrets Manager secret delivered only through the ECS execution role. */
  readonly runtimeSecretArn?: string;
}

/** Runtime resources that deployment and smoke checks consume. */
export interface ProductionRuntime {
  /** Public TLS endpoint for HTTP and realtime traffic. */
  readonly loadBalancer: elbv2.ApplicationLoadBalancer;
  /** One Fargate service that owns the HTTP API and realtime gateway. */
  readonly service: ecs.FargateService;
}

/**
 * Adds one stop-before-start Fargate service behind the production TLS ingress.
 *
 * The image tag is an explicit CloudFormation parameter so deployments select an
 * immutable image after it has been pushed to the retained control repository.
 *
 * @param stack - Cipher's disposable production runtime stack.
 * @param network - Public VPC and security boundaries from the network stack.
 * @param state - Protected application resources supplied to the server process.
 * @param control - Retained ECR repository supplying the immutable server image.
 * @param settings - Existing TLS certificate and DNS hosted-zone identifiers.
 * @returns The public load balancer and its sole service.
 */
export function addProductionRuntime(
  stack: cdk.Stack,
  network: ProductionNetwork,
  state: StateFoundations,
  control: ProductionControl,
  settings: ProductionRuntimeSettings,
): ProductionRuntime {
  addProductionTags(stack);

  const imageTag = new cdk.CfnParameter(stack, "ServerImageTag", {
    allowedPattern: "[A-Za-z0-9][A-Za-z0-9._-]{0,127}",
    default: serverImageTagDefault,
    description: "Immutable ECR tag for the Cipher server image.",
    minLength: 1,
  });
  const certificate = acm.Certificate.fromCertificateArn(
    stack,
    "Certificate",
    settings.certificateArn,
  );
  const hostedZone = route53.HostedZone.fromHostedZoneAttributes(stack, "HostedZone", {
    hostedZoneId: settings.hostedZoneId,
    zoneName: productionIngress.dns.zoneName,
  });
  const cluster = new ecs.Cluster(stack, "Cluster", {
    clusterName: "cipher-production",
    containerInsightsV2: ecs.ContainerInsights.ENABLED,
    vpc: network.vpc,
  });
  const taskDefinition = new ecs.FargateTaskDefinition(stack, "TaskDefinition", {
    cpu: 256,
    executionRole: control.executionRole,
    family: serverTaskFamily,
    memoryLimitMiB: 512,
    taskRole: control.taskRole,
  });
  const container = taskDefinition.addContainer("CipherServer", {
    environment: serverEnvironment(state),
    image: ecs.ContainerImage.fromEcrRepository(control.serverRepository, imageTag.valueAsString),
    logging: ecs.LogDrivers.awsLogs({
      logGroup: control.serverLogGroup,
      streamPrefix: serverContainerName,
    }),
  });
  addOptionalRuntimeSecret(stack, container, control, settings.runtimeSecretArn);
  container.addPortMappings({ containerPort: productionIngress.task.port });

  const service = new ecs.FargateService(stack, "Service", {
    assignPublicIp: productionIngress.task.assignPublicIp,
    availabilityZoneRebalancing: ecs.AvailabilityZoneRebalancing.DISABLED,
    cluster,
    circuitBreaker: { enable: true, rollback: true },
    desiredCount: 1,
    healthCheckGracePeriod: cdk.Duration.seconds(60),
    maxHealthyPercent: 100,
    minHealthyPercent: 0,
    platformVersion: ecs.FargatePlatformVersion.LATEST,
    securityGroups: [network.taskSecurityGroup],
    serviceName: serverServiceName,
    taskDefinition,
    vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
  });

  const loadBalancer = new elbv2.ApplicationLoadBalancer(stack, "LoadBalancer", {
    dropInvalidHeaderFields: true,
    http2Enabled: true,
    idleTimeout: cdk.Duration.seconds(300),
    internetFacing: true,
    loadBalancerName: "cipher-production",
    securityGroup: network.loadBalancerSecurityGroup,
    vpc: network.vpc,
  });
  const listener = loadBalancer.addListener("HttpsListener", {
    certificates: [certificate],
    port: productionIngress.listener.port,
    protocol: elbv2.ApplicationProtocol.HTTPS,
    sslPolicy: elbv2.SslPolicy.RECOMMENDED_TLS,
  });
  listener.addTargets("ServiceTargets", {
    deregistrationDelay: cdk.Duration.seconds(60),
    healthCheck: {
      healthyHttpCodes: "200",
      interval: cdk.Duration.seconds(30),
      path: productionIngress.endpoints.healthCheckPath,
    },
    port: productionIngress.task.port,
    protocol: elbv2.ApplicationProtocol.HTTP,
    protocolVersion: elbv2.ApplicationProtocolVersion.HTTP1,
    targets: [service.loadBalancerTarget({ containerName: "CipherServer", containerPort: 3000 })],
  });
  new route53.ARecord(stack, "PublicAlias", {
    recordName: productionIngress.dns.hostname,
    target: route53.RecordTarget.fromAlias(new route53Targets.LoadBalancerTarget(loadBalancer)),
    zone: hostedZone,
  });

  addOutput(stack, "LoadBalancerDnsName", loadBalancer.loadBalancerDnsName);
  addOutput(stack, "LoadBalancerUrl", `https://${productionIngress.dns.hostname}`);
  addOutput(stack, "ServerServiceName", service.serviceName);

  return { loadBalancer, service };
}

/**
 * Adds one optional Secrets Manager value without placing its plaintext in source or templates.
 *
 * @param stack - Stack importing the configured secret ARN.
 * @param container - Container that receives the secret through ECS at launch.
 * @param control - Control resources owning the constrained execution role.
 * @param runtimeSecretArn - Complete ARN of the optional production runtime secret.
 */
function addOptionalRuntimeSecret(
  stack: cdk.Stack,
  container: ecs.ContainerDefinition,
  control: ProductionControl,
  runtimeSecretArn: string | undefined,
): void {
  if (runtimeSecretArn === undefined) return;

  const runtimeSecret = secretsmanager.Secret.fromSecretCompleteArn(
    stack,
    "RuntimeSecret",
    runtimeSecretArn,
  );
  runtimeSecret.grantRead(control.executionRole);
  container.addSecret("CIPHER_RUNTIME_SECRET", ecs.Secret.fromSecretsManager(runtimeSecret));
}

/**
 * @param state - Protected application resources used by the server process.
 * @returns Validated server configuration from the stack-owned resources.
 */
function serverEnvironment(state: StateFoundations): Record<string, string> {
  return {
    CIPHER_API_ORIGIN: productionIngress.endpoints.apiOrigin,
    CIPHER_AWS_ACCOUNT_ID: cdk.Aws.ACCOUNT_ID,
    CIPHER_AWS_REGION: cdk.Aws.REGION,
    CIPHER_COGNITO_CLIENT_ID: state.userPoolClient.userPoolClientId,
    CIPHER_COGNITO_USER_POOL_ID: state.userPool.userPoolId,
    CIPHER_CONVERSATIONS_TABLE: state.conversationsTable.tableName,
    CIPHER_MEDIA_BUCKET: state.mediaBucket.bucketName,
    CIPHER_MEDIA_TABLE: state.mediaTable.tableName,
    CIPHER_MESSAGES_TABLE: state.messagesTable.tableName,
    CIPHER_REALTIME_URL: productionIngress.endpoints.realtimeUrl,
    CIPHER_SERVER_BIND: "0.0.0.0:3000",
    CIPHER_USERS_TABLE: state.usersTable.tableName,
    RUST_LOG: "cipher_server=info",
  };
}

/**
 * @param stack - Stack publishing the value.
 * @param id - Stable output identifier.
 * @param value - Runtime endpoint or service identifier.
 */
function addOutput(stack: cdk.Stack, id: string, value: string): void {
  new cdk.CfnOutput(stack, id, { value });
}
