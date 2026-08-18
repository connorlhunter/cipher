/** Defines Cipher's four production CloudFormation stack boundaries. */
import * as cdk from "aws-cdk-lib";
import { loadInfrastructureConfig } from "../../config/environment.js";
import { addStateFoundations } from "../lib/state-foundations.js";

const config = loadInfrastructureConfig(process.env);
const account = process.env.CDK_DEFAULT_ACCOUNT;
const region = config.awsRegion;

if (account === undefined || account.length === 0) {
  throw new Error("CDK_DEFAULT_ACCOUNT is required to synthesize Cipher infrastructure.");
}

const app = new cdk.App();
const environment = { account, region };
const allowPersistentDestruction =
  app.node.tryGetContext("cipher:allow-persistent-destruction") === "true";

const state = new cdk.Stack(app, config.stacks.state, {
  description: "Cipher production identities and encrypted application data.",
  env: environment,
  terminationProtection: !allowPersistentDestruction,
});
addStateFoundations(state);

const control = new cdk.Stack(app, config.stacks.control, {
  description: "Cipher production image, deployment identity, and retained operations data.",
  env: environment,
  terminationProtection: !allowPersistentDestruction,
});

const network = new cdk.Stack(app, config.stacks.network, {
  description: "Cipher production runtime network.",
  env: environment,
  terminationProtection: false,
});

const runtime = new cdk.Stack(app, config.stacks.runtime, {
  description: "Cipher production ingress and backend runtime.",
  env: environment,
  terminationProtection: false,
});

runtime.addStackDependency(state);
runtime.addStackDependency(control);
runtime.addStackDependency(network);
