/** Defines Cipher's persistent production identity, data, and media resources. */
import * as cdk from "aws-cdk-lib";
import * as cognito from "aws-cdk-lib/aws-cognito";
import * as dynamodb from "aws-cdk-lib/aws-dynamodb";
import * as iam from "aws-cdk-lib/aws-iam";
import * as s3 from "aws-cdk-lib/aws-s3";

/** Object-key roots reserved for the encrypted media workflow. */
export const mediaObjectPrefixes = {
  fixture: "fixtures/",
  pending: "pending/",
  ready: "ready/",
} as const;

const tableKeys = {
  partition: "pk",
  sort: "sk",
  firstIndexPartition: "gsi1pk",
  firstIndexSort: "gsi1sk",
  secondIndexPartition: "gsi2pk",
  secondIndexSort: "gsi2sk",
} as const;

const expiresAt = "expires_at";
const pointInTimeRecoveryDays = 35;
const pendingObjectLifetime = cdk.Duration.days(1);
const fixtureObjectLifetime = cdk.Duration.days(7);
const productionResourcePrefix = "cipher-production";

const resourceNames = {
  conversationsTable: `${productionResourcePrefix}-conversations`,
  mediaBucket: `${productionResourcePrefix}-media-${cdk.Aws.ACCOUNT_ID}-${cdk.Aws.REGION}`,
  mediaTable: `${productionResourcePrefix}-media`,
  nativeClient: `${productionResourcePrefix}-desktop`,
  userPool: `${productionResourcePrefix}-users`,
  usersTable: `${productionResourcePrefix}-users`,
} as const;

/** Persistent resources that later stacks and runtime configuration consume. */
export interface StateFoundations {
  /** Public Cognito user pool for native Cipher sign-in. */
  readonly userPool: cognito.UserPool;
  /** Native Cognito client with no secret or browser OAuth configuration. */
  readonly userPoolClient: cognito.UserPoolClient;
  /** User profiles, identity claims, relationships, devices, and sessions. */
  readonly usersTable: dynamodb.Table;
  /** Conversations, memberships, servers, roles, and navigation records. */
  readonly conversationsTable: dynamodb.Table;
  /** Ciphertext messages and idempotency records. */
  readonly messagesTable: dynamodb.Table;
  /** Metadata for private client-encrypted media. */
  readonly mediaTable: dynamodb.Table;
  /** Private bucket for client-encrypted media ciphertext. */
  readonly mediaBucket: s3.Bucket;
}

/**
 * Adds the state stack resources required before the backend and network stacks exist.
 *
 * @param stack - Cipher's production state stack.
 * @returns Resources whose deployed outputs fill the runtime environment.
 */
export function addStateFoundations(stack: cdk.Stack): StateFoundations {
  const userPool = new cognito.UserPool(stack, "UserPool", {
    accountRecovery: cognito.AccountRecovery.EMAIL_ONLY,
    autoVerify: { email: true },
    deletionProtection: true,
    email: cognito.UserPoolEmail.withCognito(),
    mfa: cognito.Mfa.OPTIONAL,
    mfaSecondFactor: { email: false, otp: true, sms: false },
    passwordPolicy: {
      minLength: 12,
      requireDigits: true,
      requireLowercase: true,
      requireSymbols: true,
      requireUppercase: true,
    },
    removalPolicy: cdk.RemovalPolicy.RETAIN,
    selfSignUpEnabled: false,
    signInAliases: { email: true },
    standardAttributes: { email: { mutable: true, required: true } },
    userPoolName: resourceNames.userPool,
  });

  const userPoolClient = userPool.addClient("NativePublicClient", {
    accessTokenValidity: cdk.Duration.hours(1),
    authFlows: { userSrp: true },
    disableOAuth: true,
    enableTokenRevocation: true,
    generateSecret: false,
    idTokenValidity: cdk.Duration.hours(1),
    preventUserExistenceErrors: true,
    refreshTokenValidity: cdk.Duration.days(30),
    supportedIdentityProviders: [cognito.UserPoolClientIdentityProvider.COGNITO],
    userPoolClientName: resourceNames.nativeClient,
  });

  const usersTable = createTable(stack, "Users", resourceNames.usersTable);
  const conversationsTable = createTable(stack, "Conversations", resourceNames.conversationsTable);
  conversationsTable.addGlobalSecondaryIndex({
    indexName: "GSI1",
    partitionKey: { name: tableKeys.firstIndexPartition, type: dynamodb.AttributeType.STRING },
    sortKey: { name: tableKeys.firstIndexSort, type: dynamodb.AttributeType.STRING },
  });

  const messagesTable = createTable(stack, "Messages", `${productionResourcePrefix}-messages`);
  messagesTable.addGlobalSecondaryIndex({
    indexName: "GSI1",
    partitionKey: { name: tableKeys.firstIndexPartition, type: dynamodb.AttributeType.STRING },
    sortKey: { name: tableKeys.firstIndexSort, type: dynamodb.AttributeType.STRING },
  });

  const mediaTable = createTable(stack, "Media", resourceNames.mediaTable);
  mediaTable.addGlobalSecondaryIndex({
    indexName: "GSI1",
    partitionKey: { name: tableKeys.firstIndexPartition, type: dynamodb.AttributeType.STRING },
    sortKey: { name: tableKeys.firstIndexSort, type: dynamodb.AttributeType.STRING },
  });
  mediaTable.addGlobalSecondaryIndex({
    indexName: "GSI2",
    partitionKey: { name: tableKeys.secondIndexPartition, type: dynamodb.AttributeType.STRING },
    sortKey: { name: tableKeys.secondIndexSort, type: dynamodb.AttributeType.STRING },
  });

  const mediaBucket = new s3.Bucket(stack, "MediaBucket", {
    bucketName: resourceNames.mediaBucket,
    blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
    encryption: s3.BucketEncryption.S3_MANAGED,
    enforceSSL: true,
    lifecycleRules: [
      {
        abortIncompleteMultipartUploadAfter: pendingObjectLifetime,
        expiration: pendingObjectLifetime,
        id: "ExpirePendingCiphertext",
        prefix: mediaObjectPrefixes.pending,
      },
      {
        expiration: fixtureObjectLifetime,
        id: "ExpireFixtureCiphertext",
        prefix: mediaObjectPrefixes.fixture,
      },
    ],
    objectOwnership: s3.ObjectOwnership.BUCKET_OWNER_ENFORCED,
    removalPolicy: cdk.RemovalPolicy.RETAIN,
  });
  addMediaBucketGuards(mediaBucket);

  addOutput(stack, "CognitoUserPoolId", userPool.userPoolId);
  addOutput(stack, "CognitoUserPoolClientId", userPoolClient.userPoolClientId);
  addOutput(stack, "UsersTableName", usersTable.tableName);
  addOutput(stack, "ConversationsTableName", conversationsTable.tableName);
  addOutput(stack, "MessagesTableName", messagesTable.tableName);
  addOutput(stack, "MediaTableName", mediaTable.tableName);
  addOutput(stack, "MediaBucketName", mediaBucket.bucketName);
  addOutput(stack, "MediaPendingPrefix", mediaObjectPrefixes.pending);
  addOutput(stack, "MediaReadyPrefix", mediaObjectPrefixes.ready);
  addOutput(stack, "MediaFixturePrefix", mediaObjectPrefixes.fixture);

  return {
    conversationsTable,
    mediaBucket,
    mediaTable,
    messagesTable,
    userPool,
    userPoolClient,
    usersTable,
  };
}

/**
 * @param stack - Stack owning the table.
 * @param id - Stable construct identifier.
 * @param tableName - Stable production name for the table.
 * @returns An on-demand, encrypted, protected table with TTL enabled.
 */
function createTable(stack: cdk.Stack, id: string, tableName: string): dynamodb.Table {
  return new dynamodb.Table(stack, id, {
    billingMode: dynamodb.BillingMode.PAY_PER_REQUEST,
    deletionProtection: true,
    encryption: dynamodb.TableEncryption.AWS_MANAGED,
    partitionKey: { name: tableKeys.partition, type: dynamodb.AttributeType.STRING },
    pointInTimeRecoverySpecification: {
      pointInTimeRecoveryEnabled: true,
      recoveryPeriodInDays: pointInTimeRecoveryDays,
    },
    removalPolicy: cdk.RemovalPolicy.RETAIN,
    sortKey: { name: tableKeys.sort, type: dynamodb.AttributeType.STRING },
    tableName,
    timeToLiveAttribute: expiresAt,
  });
}

/**
 * @param bucket - Bucket whose data-plane invariants must be enforced centrally.
 */
function addMediaBucketGuards(bucket: s3.Bucket): void {
  bucket.addToResourcePolicy(
    new iam.PolicyStatement({
      actions: ["s3:PutObject"],
      conditions: { Null: { "s3:x-amz-server-side-encryption": "true" } },
      effect: iam.Effect.DENY,
      principals: [new iam.AnyPrincipal()],
      resources: [bucket.arnForObjects("*")],
      sid: "DenyMissingS3ManagedEncryption",
    }),
  );
  bucket.addToResourcePolicy(
    new iam.PolicyStatement({
      actions: ["s3:PutObject"],
      conditions: { StringNotEquals: { "s3:x-amz-server-side-encryption": "AES256" } },
      effect: iam.Effect.DENY,
      principals: [new iam.AnyPrincipal()],
      resources: [bucket.arnForObjects("*")],
      sid: "DenyWrongS3ManagedEncryption",
    }),
  );
  bucket.addToResourcePolicy(
    new iam.PolicyStatement({
      actions: ["s3:PutObject"],
      effect: iam.Effect.DENY,
      notResources: [
        bucket.arnForObjects(`${mediaObjectPrefixes.pending}*`),
        bucket.arnForObjects(`${mediaObjectPrefixes.ready}*`),
        bucket.arnForObjects(`${mediaObjectPrefixes.fixture}*`),
      ],
      principals: [new iam.AnyPrincipal()],
      sid: "DenyWritesOutsideCipherPrefixes",
    }),
  );
}

/**
 * @param stack - Stack publishing the value.
 * @param id - Output identifier.
 * @param value - Deployed value for later runtime configuration.
 */
function addOutput(stack: cdk.Stack, id: string, value: string): void {
  new cdk.CfnOutput(stack, id, { value });
}
