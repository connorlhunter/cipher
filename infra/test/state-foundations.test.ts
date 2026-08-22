import assert from "node:assert/strict";
import { describe, test } from "node:test";

import * as cdk from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";

import { addStateFoundations } from "../lib/state-foundations.js";

interface CloudFormationResource {
  readonly DeletionPolicy?: unknown;
  readonly Properties?: Record<string, unknown>;
  readonly UpdateReplacePolicy?: unknown;
}

interface CloudFormationOutput {
  readonly Value?: unknown;
}

function stateTemplate(allowDestruction = false): Template {
  const app = new cdk.App();
  const stack = new cdk.Stack(app, "State", {
    env: { account: "123456789012", region: "us-east-1" },
  });
  addStateFoundations(stack, { allowDestruction });
  return Template.fromStack(stack);
}

function onlyResource(template: Template, type: string): CloudFormationResource {
  const resources = Object.values(template.findResources(type)) as CloudFormationResource[];
  assert.equal(resources.length, 1, `expected one ${type} resource`);
  return resources[0] as CloudFormationResource;
}

function properties(resource: CloudFormationResource): Record<string, unknown> {
  assert.ok(resource.Properties, "expected CloudFormation resource properties");
  return resource.Properties;
}

function secondaryIndexes(resource: CloudFormationResource): Record<string, unknown>[] {
  const indexes = properties(resource).GlobalSecondaryIndexes;
  if (indexes === undefined) {
    return [];
  }
  assert.ok(Array.isArray(indexes), "expected global secondary indexes to be an array");
  return indexes as Record<string, unknown>[];
}

function indexByName(resource: CloudFormationResource, name: string): Record<string, unknown> {
  const index = secondaryIndexes(resource).find((candidate) => candidate.IndexName === name);
  assert.ok(index, `expected ${name}`);
  return index;
}

function statementBySid(
  statements: Record<string, unknown>[],
  sid: string,
): Record<string, unknown> {
  const statement = statements.find((candidate) => candidate.Sid === sid);
  assert.ok(statement, `expected bucket policy statement ${sid}`);
  return statement;
}

function outputValues(template: Template): Record<string, CloudFormationOutput> {
  const document = template.toJSON() as { readonly Outputs?: Record<string, CloudFormationOutput> };
  assert.ok(document.Outputs, "expected state stack outputs");
  return document.Outputs;
}

describe("Cipher state foundations", () => {
  test("synthesizes an invite-only Cognito pool and a native SRP client", () => {
    const template = stateTemplate();
    const pool = properties(onlyResource(template, "AWS::Cognito::UserPool"));
    const client = properties(onlyResource(template, "AWS::Cognito::UserPoolClient"));

    assert.deepEqual(pool.AdminCreateUserConfig, { AllowAdminCreateUserOnly: true });
    assert.deepEqual(pool.AutoVerifiedAttributes, ["email"]);
    assert.equal(pool.DeletionProtection, "ACTIVE");
    assert.deepEqual(pool.EmailConfiguration, { EmailSendingAccount: "COGNITO_DEFAULT" });
    assert.deepEqual(pool.EnabledMfas, ["SOFTWARE_TOKEN_MFA"]);
    assert.equal(pool.MfaConfiguration, "OPTIONAL");
    assert.deepEqual(pool.AccountRecoverySetting, {
      RecoveryMechanisms: [{ Name: "verified_email", Priority: 1 }],
    });
    assert.deepEqual(pool.UsernameAttributes, ["email"]);
    assert.equal(pool.UserPoolName, "cipher-production-users");
    assert.deepEqual(pool.Policies, {
      PasswordPolicy: {
        MinimumLength: 12,
        RequireLowercase: true,
        RequireNumbers: true,
        RequireSymbols: true,
        RequireUppercase: true,
      },
    });

    assert.equal(client.GenerateSecret, false);
    assert.equal(client.AllowedOAuthFlowsUserPoolClient, false);
    assert.equal("CallbackURLs" in client, false);
    assert.equal("LogoutURLs" in client, false);
    assert.deepEqual(client.ExplicitAuthFlows, ["ALLOW_USER_SRP_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"]);
    assert.equal(client.EnableTokenRevocation, true);
    assert.equal(client.PreventUserExistenceErrors, "ENABLED");
    assert.deepEqual(client.SupportedIdentityProviders, ["COGNITO"]);
    assert.equal(client.AccessTokenValidity, 60);
    assert.equal(client.IdTokenValidity, 60);
    assert.equal(client.RefreshTokenValidity, 43200);
    assert.equal(client.ClientName, "cipher-production-desktop");
  });

  test("synthesizes only the documented tables, keys, and indexes with protected data controls", () => {
    const template = stateTemplate();
    const tables = Object.values(
      template.findResources("AWS::DynamoDB::Table"),
    ) as CloudFormationResource[];
    const users = tables.find((table) => secondaryIndexes(table).length === 0);
    const navigationTables = tables.filter((table) => secondaryIndexes(table).length === 1);
    const media = tables.find((table) => secondaryIndexes(table).length === 2);

    assert.equal(tables.length, 4);
    assert.ok(users, "expected the Users table without a navigation index");
    assert.equal(navigationTables.length, 2);
    assert.ok(media, "expected the Media table with owner and resource indexes");
    for (const table of [users, ...navigationTables, media]) {
      const tableProperties = properties(table);
      assert.equal(table.DeletionPolicy, "Retain");
      assert.equal(table.UpdateReplacePolicy, "Retain");
      assert.equal(tableProperties.BillingMode, "PAY_PER_REQUEST");
      assert.equal(tableProperties.DeletionProtectionEnabled, true);
      assert.deepEqual(tableProperties.SSESpecification, { SSEEnabled: true });
      assert.deepEqual(tableProperties.PointInTimeRecoverySpecification, {
        PointInTimeRecoveryEnabled: true,
        RecoveryPeriodInDays: 35,
      });
      assert.deepEqual(tableProperties.TimeToLiveSpecification, {
        AttributeName: "expires_at",
        Enabled: true,
      });
      assert.deepEqual(tableProperties.KeySchema, [
        { AttributeName: "pk", KeyType: "HASH" },
        { AttributeName: "sk", KeyType: "RANGE" },
      ]);
      assert.ok(
        [
          "cipher-production-users",
          "cipher-production-conversations",
          "cipher-production-messages",
          "cipher-production-media",
        ].includes(tableProperties.TableName as string),
      );
    }

    assert.deepEqual(secondaryIndexes(users), []);
    for (const table of navigationTables) {
      assert.deepEqual(indexByName(table, "GSI1"), {
        IndexName: "GSI1",
        KeySchema: [
          { AttributeName: "gsi1pk", KeyType: "HASH" },
          { AttributeName: "gsi1sk", KeyType: "RANGE" },
        ],
        Projection: { ProjectionType: "ALL" },
      });
    }
    assert.deepEqual(indexByName(media, "GSI1"), {
      IndexName: "GSI1",
      KeySchema: [
        { AttributeName: "gsi1pk", KeyType: "HASH" },
        { AttributeName: "gsi1sk", KeyType: "RANGE" },
      ],
      Projection: { ProjectionType: "ALL" },
    });
    assert.deepEqual(indexByName(media, "GSI2"), {
      IndexName: "GSI2",
      KeySchema: [
        { AttributeName: "gsi2pk", KeyType: "HASH" },
        { AttributeName: "gsi2sk", KeyType: "RANGE" },
      ],
      Projection: { ProjectionType: "ALL" },
    });
  });

  test("keeps media ciphertext private, SSE-S3-bound, prefix-bound, and lifecycle-managed", () => {
    const template = stateTemplate();
    const bucket = onlyResource(template, "AWS::S3::Bucket");
    const bucketProperties = properties(bucket);
    const bucketPolicy = properties(onlyResource(template, "AWS::S3::BucketPolicy"));
    const policyDocument = bucketPolicy.PolicyDocument as { Statement?: Record<string, unknown>[] };
    const statements = policyDocument.Statement;

    assert.equal(bucket.DeletionPolicy, "Retain");
    assert.equal(bucket.UpdateReplacePolicy, "Retain");
    assert.deepEqual(bucketProperties.BucketName, {
      "Fn::Join": [
        "",
        ["cipher-production-media-", { Ref: "AWS::AccountId" }, "-", { Ref: "AWS::Region" }],
      ],
    });
    assert.deepEqual(bucketProperties.PublicAccessBlockConfiguration, {
      BlockPublicAcls: true,
      BlockPublicPolicy: true,
      IgnorePublicAcls: true,
      RestrictPublicBuckets: true,
    });
    assert.deepEqual(bucketProperties.BucketEncryption, {
      ServerSideEncryptionConfiguration: [
        { ServerSideEncryptionByDefault: { SSEAlgorithm: "AES256" } },
      ],
    });
    assert.deepEqual(bucketProperties.OwnershipControls, {
      Rules: [{ ObjectOwnership: "BucketOwnerEnforced" }],
    });
    assert.deepEqual(bucketProperties.VersioningConfiguration, { Status: "Enabled" });
    assert.deepEqual(bucketProperties.LifecycleConfiguration, {
      Rules: [
        {
          Id: "ExpireNoncurrentCiphertextVersions",
          NoncurrentVersionExpiration: { NoncurrentDays: 35 },
          Status: "Enabled",
        },
        {
          AbortIncompleteMultipartUpload: { DaysAfterInitiation: 1 },
          ExpirationInDays: 1,
          Id: "ExpirePendingCiphertext",
          Prefix: "pending/",
          Status: "Enabled",
        },
        {
          ExpirationInDays: 7,
          Id: "ExpireFixtureCiphertext",
          Prefix: "fixtures/",
          Status: "Enabled",
        },
      ],
    });
    assert.equal("CorsConfiguration" in bucketProperties, false);
    assert.equal("WebsiteConfiguration" in bucketProperties, false);

    assert.ok(statements, "expected a media bucket policy");
    assert.equal(
      statements.some((statement) => statement.Effect === "Allow"),
      false,
    );
    assert.ok(
      statements.some(
        (statement) =>
          JSON.stringify(statement.Condition) ===
          JSON.stringify({ Bool: { "aws:SecureTransport": "false" } }),
      ),
    );
    assert.deepEqual(statementBySid(statements, "DenyMissingPayloadChecksum").Condition, {
      Null: { "s3:x-amz-content-sha256": "true" },
    });
    assert.deepEqual(statementBySid(statements, "DenyUnsignedPayload").Condition, {
      StringEquals: { "s3:x-amz-content-sha256": "UNSIGNED-PAYLOAD" },
    });
    assert.deepEqual(statementBySid(statements, "DenyMissingS3ManagedEncryption").Condition, {
      Null: { "s3:x-amz-server-side-encryption": "true" },
    });
    assert.deepEqual(statementBySid(statements, "DenyWrongS3ManagedEncryption").Condition, {
      StringNotEquals: { "s3:x-amz-server-side-encryption": "AES256" },
    });
    const prefixGuard = statementBySid(statements, "DenyWritesOutsideCipherPrefixes");
    assert.equal(prefixGuard.Effect, "Deny");
    assert.equal(prefixGuard.Action, "s3:PutObject");
    const prefixGuardJson = JSON.stringify(prefixGuard.NotResource);
    for (const prefix of ["pending/*", "ready/*", "fixtures/*"]) {
      assert.ok(prefixGuardJson.includes(prefix), `expected ${prefix} in the key guard`);
    }
  });

  test("switches every persistent state resource to destructive mode only when requested", () => {
    const template = stateTemplate(true);
    const userPool = onlyResource(template, "AWS::Cognito::UserPool");
    const bucket = onlyResource(template, "AWS::S3::Bucket");
    const tables = Object.values(
      template.findResources("AWS::DynamoDB::Table"),
    ) as CloudFormationResource[];
    const resources = template.toJSON() as {
      readonly Resources?: Record<string, { readonly Type?: unknown }>;
    };

    assert.equal(userPool.DeletionPolicy, "Delete");
    assert.equal(userPool.UpdateReplacePolicy, "Delete");
    assert.equal(properties(userPool).DeletionProtection, "INACTIVE");
    assert.equal(bucket.DeletionPolicy, "Delete");
    assert.equal(bucket.UpdateReplacePolicy, "Delete");
    assert.equal(tables.length, 4);
    for (const table of tables) {
      assert.equal(table.DeletionPolicy, "Delete");
      assert.equal(table.UpdateReplacePolicy, "Delete");
      assert.equal(properties(table).DeletionProtectionEnabled, false);
    }
    assert.ok(
      Object.values(resources.Resources ?? {}).some(
        (resource) => resource.Type === "Custom::S3AutoDeleteObjects",
      ),
      "expected the destructive synthesis to empty versioned media objects before deletion",
    );
  });

  test("publishes the authoritative values needed for runtime configuration", () => {
    const outputs = outputValues(stateTemplate());

    for (const output of [
      "CognitoUserPoolId",
      "CognitoUserPoolClientId",
      "UsersTableName",
      "ConversationsTableName",
      "MessagesTableName",
      "MediaTableName",
      "MediaBucketName",
      "MediaPendingPrefix",
      "MediaReadyPrefix",
      "MediaFixturePrefix",
    ]) {
      assert.ok(outputs[output], `expected ${output}`);
    }
    assert.equal(outputs.MediaPendingPrefix?.Value, "pending/");
    assert.equal(outputs.MediaReadyPrefix?.Value, "ready/");
    assert.equal(outputs.MediaFixturePrefix?.Value, "fixtures/");
  });
});
