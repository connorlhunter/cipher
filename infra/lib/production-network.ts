/** Defines Cipher's single production VPC and its network trust boundaries. */
import * as cdk from "aws-cdk-lib";
import * as ec2 from "aws-cdk-lib/aws-ec2";

import { productionIngress } from "./production-ingress.js";

const productionNetworkCidr = "10.72.0.0/16";
const productionResourcePrefix = "cipher-production";

/** Tags applied to every production network resource for ownership and cost allocation. */
export const productionNetworkTags = {
  Application: "cipher",
  CostCenter: "cipher-production",
  Environment: "production",
  ManagedBy: "cdk",
} as const;

/** Shared network resources consumed by the production runtime stack. */
export interface ProductionNetwork {
  /** Two-AZ IPv4 VPC containing the public service subnets. */
  readonly vpc: ec2.Vpc;
  /** Only public ingress boundary; the runtime attaches this to its ALB. */
  readonly loadBalancerSecurityGroup: ec2.SecurityGroup;
  /** Runtime boundary: it accepts service traffic only from the ALB boundary. */
  readonly taskSecurityGroup: ec2.SecurityGroup;
}

/**
 * Adds the bounded network foundation for Cipher's one production environment.
 *
 * The closed alpha deliberately uses two public subnets and public task addresses
 * instead of NAT gateways. The task security group still prevents direct public
 * access; only the later ALB can reach its HTTP port.
 *
 * @param stack - The Cipher production network stack.
 * @returns Network resources for the production runtime stack.
 */
export function addProductionNetwork(stack: cdk.Stack): ProductionNetwork {
  addProductionTags(stack);

  const vpc = new ec2.Vpc(stack, "Vpc", {
    enableDnsHostnames: true,
    enableDnsSupport: true,
    ipAddresses: ec2.IpAddresses.cidr(productionNetworkCidr),
    maxAzs: productionIngress.availabilityZones,
    natGateways: productionIngress.natGateways,
    restrictDefaultSecurityGroup: true,
    subnetConfiguration: [
      {
        cidrMask: 24,
        name: "public",
        subnetType: ec2.SubnetType.PUBLIC,
      },
    ],
    vpcName: `${productionResourcePrefix}-network`,
  });

  const loadBalancerSecurityGroup = new ec2.SecurityGroup(stack, "LoadBalancerSecurityGroup", {
    allowAllOutbound: false,
    description: "Accepts Cipher production TLS and forwards only to the service task.",
    securityGroupName: `${productionResourcePrefix}-ingress`,
    vpc,
  });
  const taskSecurityGroup = new ec2.SecurityGroup(stack, "TaskSecurityGroup", {
    allowAllOutbound: false,
    description: "Accepts Cipher service traffic only from the production load balancer.",
    securityGroupName: `${productionResourcePrefix}-service`,
    vpc,
  });

  loadBalancerSecurityGroup.addIngressRule(
    ec2.Peer.anyIpv4(),
    ec2.Port.tcp(productionIngress.listener.port),
    "Public Cipher HTTPS ingress",
  );
  loadBalancerSecurityGroup.addEgressRule(
    taskSecurityGroup,
    ec2.Port.tcp(productionIngress.task.port),
    "Cipher HTTP service target",
  );
  taskSecurityGroup.addIngressRule(
    loadBalancerSecurityGroup,
    ec2.Port.tcp(productionIngress.task.port),
    "Cipher load balancer target traffic",
  );
  addTaskEgressRules(taskSecurityGroup);

  addOutput(stack, "VpcId", vpc.vpcId);
  addOutput(stack, "LoadBalancerSecurityGroupId", loadBalancerSecurityGroup.securityGroupId);
  addOutput(stack, "TaskSecurityGroupId", taskSecurityGroup.securityGroupId);

  return { loadBalancerSecurityGroup, taskSecurityGroup, vpc };
}

/**
 * @param stack - Stack whose resources receive the production allocation tags.
 */
function addProductionTags(stack: cdk.Stack): void {
  for (const [key, value] of Object.entries(productionNetworkTags)) {
    cdk.Tags.of(stack).add(key, value);
  }
}

/**
 * Grants the service only the outbound ports required to resolve and call AWS APIs.
 *
 * The backend needs HTTPS for Cognito, DynamoDB, S3, ECR, and CloudWatch. DNS is
 * kept explicit so the public task address never turns into a general-purpose
 * outbound network boundary.
 *
 * @param taskSecurityGroup - Production service security group.
 */
function addTaskEgressRules(taskSecurityGroup: ec2.SecurityGroup): void {
  taskSecurityGroup.addEgressRule(
    ec2.Peer.anyIpv4(),
    ec2.Port.tcp(443),
    "AWS API and image-delivery TLS",
  );
  taskSecurityGroup.addEgressRule(ec2.Peer.anyIpv4(), ec2.Port.tcp(53), "DNS over TCP");
  taskSecurityGroup.addEgressRule(ec2.Peer.anyIpv4(), ec2.Port.udp(53), "DNS over UDP");
}

/**
 * @param stack - Stack publishing a runtime configuration value.
 * @param id - Stable output identifier.
 * @param value - CloudFormation value for the later runtime stack.
 */
function addOutput(stack: cdk.Stack, id: string, value: string): void {
  new cdk.CfnOutput(stack, id, { value });
}
