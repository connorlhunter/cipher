/** Defines Cipher's four production CloudFormation stack boundaries. */
import * as cdk from "aws-cdk-lib";
import { productionConfig } from "../../config/production.js";

const productionRegion = productionConfig.awsRegion;
const account = process.env.CDK_DEFAULT_ACCOUNT;
const region = process.env.CIPHER_AWS_REGION ?? productionRegion;

if (account === undefined || account.length === 0) {
  throw new Error("CDK_DEFAULT_ACCOUNT is required to synthesize Cipher infrastructure.");
}

if (region !== productionRegion) {
  throw new Error(`Cipher infrastructure must use ${productionRegion}.`);
}

const app = new cdk.App();
const environment = { account, region };
const allowPersistentDestruction =
  app.node.tryGetContext("cipher:allow-persistent-destruction") === "true";

const state = new cdk.Stack(app, productionConfig.stacks.state, {
  description: "Cipher production identities and encrypted application data.",
  env: environment,
  terminationProtection: !allowPersistentDestruction,
});

const control = new cdk.Stack(app, productionConfig.stacks.control, {
  description: "Cipher production image, deployment identity, and retained operations data.",
  env: environment,
  terminationProtection: !allowPersistentDestruction,
});

const network = new cdk.Stack(app, productionConfig.stacks.network, {
  description: "Cipher production runtime network.",
  env: environment,
  terminationProtection: false,
});

const runtime = new cdk.Stack(app, productionConfig.stacks.runtime, {
  description: "Cipher production ingress and backend runtime.",
  env: environment,
  terminationProtection: false,
});

runtime.addStackDependency(state);
runtime.addStackDependency(control);
runtime.addStackDependency(network);
