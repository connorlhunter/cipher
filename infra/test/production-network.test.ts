import assert from "node:assert/strict";
import { describe, test } from "node:test";

import * as cdk from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";

import { addProductionNetwork, productionNetworkTags } from "../lib/production-network.js";

interface CloudFormationResource {
  readonly Properties?: Record<string, unknown>;
}

function networkTemplate(): Template {
  const app = new cdk.App();
  const stack = new cdk.Stack(app, "Network", {
    env: { account: "123456789012", region: "us-east-1" },
  });
  addProductionNetwork(stack);
  return Template.fromStack(stack);
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

function securityGroup(template: Template, name: string): CloudFormationResource {
  const match = resources(template, "AWS::EC2::SecurityGroup").find(
    (resource) => properties(resource).GroupName === name,
  );
  assert.ok(match, `expected security group ${name}`);
  return match;
}

function outputs(template: Template): Record<string, unknown> {
  const document = template.toJSON() as { readonly Outputs?: Record<string, unknown> };
  assert.ok(document.Outputs, "expected stack outputs");
  return document.Outputs;
}

describe("Cipher production network", () => {
  test("creates one tagged two-AZ public VPC with no NAT or endpoint cost", () => {
    const template = networkTemplate();
    const vpc = onlyResource(template, "AWS::EC2::VPC");
    const vpcProperties = properties(vpc);
    const subnets = resources(template, "AWS::EC2::Subnet");
    const routes = resources(template, "AWS::EC2::Route");

    assert.equal(vpcProperties.CidrBlock, "10.72.0.0/16");
    assert.equal(vpcProperties.EnableDnsHostnames, true);
    assert.equal(vpcProperties.EnableDnsSupport, true);
    assert.deepEqual(tags(vpc), {
      ...productionNetworkTags,
      Name: "cipher-production-network",
    });
    assert.equal(subnets.length, 2);
    assert.deepEqual(subnets.map((subnet) => properties(subnet).CidrBlock).sort(), [
      "10.72.0.0/24",
      "10.72.1.0/24",
    ]);
    for (const subnet of subnets) {
      assert.equal(properties(subnet).MapPublicIpOnLaunch, true);
      assert.equal(tags(subnet)["aws-cdk:subnet-name"], "public");
      assert.equal(tags(subnet)["aws-cdk:subnet-type"], "Public");
      for (const [key, value] of Object.entries(productionNetworkTags)) {
        assert.equal(tags(subnet)[key], value);
      }
    }
    assert.equal(resources(template, "AWS::EC2::InternetGateway").length, 1);
    assert.equal(routes.length, 2);
    for (const route of routes) {
      assert.equal(properties(route).DestinationCidrBlock, "0.0.0.0/0");
      assert.ok(
        "GatewayId" in properties(route),
        "expected each public route to use the internet gateway",
      );
    }
    assert.equal(resources(template, "AWS::EC2::NatGateway").length, 0);
    assert.equal(resources(template, "AWS::EC2::EIP").length, 0);
    assert.equal(resources(template, "AWS::EC2::VPCEndpoint").length, 0);
    assert.equal(resources(template, "Custom::VpcRestrictDefaultSG").length, 1);
  });

  test("makes the load balancer boundary the only public path to the service", () => {
    const template = networkTemplate();
    const ingress = securityGroup(template, "cipher-production-ingress");
    const service = securityGroup(template, "cipher-production-service");
    const ingressProperties = properties(ingress);
    const serviceProperties = properties(service);
    const publicIngress = ingressProperties.SecurityGroupIngress;
    const serviceEgress = serviceProperties.SecurityGroupEgress;
    const loadBalancerEgress = onlyResource(template, "AWS::EC2::SecurityGroupEgress");
    const serviceIngress = onlyResource(template, "AWS::EC2::SecurityGroupIngress");

    assert.ok(Array.isArray(publicIngress), "expected HTTPS ingress rule");
    assert.deepEqual(publicIngress, [
      {
        CidrIp: "0.0.0.0/0",
        Description: "Public Cipher HTTPS ingress",
        FromPort: 443,
        IpProtocol: "tcp",
        ToPort: 443,
      },
    ]);
    assert.equal("SecurityGroupIngress" in serviceProperties, false);
    assert.ok(Array.isArray(serviceEgress), "expected bounded service egress rules");
    assert.deepEqual(
      serviceEgress.map((rule) => {
        assert.ok(typeof rule === "object" && rule !== null, "expected egress rule");
        const value = rule as Record<string, unknown>;
        return [value.IpProtocol, value.FromPort, value.ToPort, value.CidrIp];
      }),
      [
        ["tcp", 443, 443, "0.0.0.0/0"],
        ["tcp", 53, 53, "0.0.0.0/0"],
        ["udp", 53, 53, "0.0.0.0/0"],
      ],
    );

    const loadBalancerEgressProperties = properties(loadBalancerEgress);
    assert.equal(loadBalancerEgressProperties.IpProtocol, "tcp");
    assert.equal(loadBalancerEgressProperties.FromPort, 3000);
    assert.equal(loadBalancerEgressProperties.ToPort, 3000);
    assert.ok(
      "DestinationSecurityGroupId" in loadBalancerEgressProperties,
      "expected load balancer egress to target only the service group",
    );

    const serviceIngressProperties = properties(serviceIngress);
    assert.equal(serviceIngressProperties.IpProtocol, "tcp");
    assert.equal(serviceIngressProperties.FromPort, 3000);
    assert.equal(serviceIngressProperties.ToPort, 3000);
    assert.ok(
      "SourceSecurityGroupId" in serviceIngressProperties,
      "expected service ingress to come only from the load balancer group",
    );
    assert.equal("CidrIp" in serviceIngressProperties, false);

    for (const resource of [ingress, service]) {
      for (const [key, value] of Object.entries(productionNetworkTags)) {
        assert.equal(tags(resource)[key], value);
      }
    }
  });

  test("publishes the exact cross-stack runtime values", () => {
    const networkOutputs = outputs(networkTemplate());

    for (const output of ["VpcId", "LoadBalancerSecurityGroupId", "TaskSecurityGroupId"]) {
      assert.ok(networkOutputs[output], `expected ${output}`);
    }
  });
});
