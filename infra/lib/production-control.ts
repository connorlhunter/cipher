/** Defines the retained image repository that supplies Cipher's production runtime. */
import * as cdk from "aws-cdk-lib";
import * as backup from "aws-cdk-lib/aws-backup";
import * as budgets from "aws-cdk-lib/aws-budgets";
import * as ecr from "aws-cdk-lib/aws-ecr";
import * as iam from "aws-cdk-lib/aws-iam";
import * as logs from "aws-cdk-lib/aws-logs";

import { addProductionTags } from "./production-network.js";
import type { StateFoundations } from "./state-foundations.js";

const productionRepositoryName = "cipher-production-server";
const productionBackupVaultName = "cipher-production-recovery";
const productionBudgetName = "cipher-production-monthly";
const productionDeploymentRoleName = "cipher-production-deployment";
const productionExecutionRoleName = "cipher-production-execution";
const productionTaskRoleName = "cipher-production-task";
const productionBackupRoleName = "cipher-production-AWSBackup";
const githubOidcProviderUrl = "https://token.actions.githubusercontent.com";
// GitHub's immutable subject format binds this trust to Cipher's stable owner and repository IDs.
const githubDeploymentSubject =
  "repo:connorlhunter@59103082/cipher@1333508685:environment:production";
const cdkBootstrapQualifier = "hnb659fds";
const cdkBootstrapStackName = "CDKToolkit";
const productionLogGroupName = "/cipher/production/server";
const productionClusterName = "cipher-production";
const productionServiceName = "cipher-production-server";

/** Image resources retained while the runtime and network are paused. */
export interface ProductionControl {
  /** Dedicated immutable repository for the one Cipher server image. */
  readonly serverRepository: ecr.Repository;
  /** Role trusted only by the protected production deployment environment. */
  readonly deploymentRole: iam.Role;
  /** Role that can pull the server image and write the bounded server logs. */
  readonly executionRole: iam.Role;
  /** Role intentionally empty until the server needs a narrowly scoped AWS data-plane action. */
  readonly taskRole: iam.Role;
  /** AWS Backup role used for bounded recovery points of the persistent tables. */
  readonly backupRole: iam.Role;
  /** AWS-managed-key recovery vault for pre-deployment and scheduled table backups. */
  readonly backupVault: backup.BackupVault;
  /** Retained server log group used by runtime tasks and pause/resume diagnostics. */
  readonly serverLogGroup: logs.LogGroup;
}

/** Exact CloudFormation stack names that the protected deployment path may inspect. */
export interface ProductionStackNames {
  /** Retained deployment, image, backup, and log controls. */
  readonly control: string;
  /** Disposable public network boundary. */
  readonly network: string;
  /** Disposable ECS service and ingress. */
  readonly runtime: string;
  /** Protected identity and application state. */
  readonly state: string;
}

/** Controls supplied from deployment configuration rather than source code. */
export interface ProductionControlSettings {
  /** Address that receives the production cost budget notifications. */
  readonly budgetAlertEmail: string;
  /** Enables removal only while the fully confirmed teardown flow is running. */
  readonly allowDestruction?: boolean;
  /** Only these four Cipher stacks may be read by the deployment workflow. */
  readonly stackNames: ProductionStackNames;
}

/**
 * Adds the production repository before any runtime task is created.
 *
 * The repository belongs to the protected control stack so a pause can remove
 * hourly network and runtime resources without losing the image used to resume.
 *
 * @param stack - Cipher's protected production control stack.
 * @param state - Persistent resources covered by recovery and fixture operations.
 * @param settings - Deployment notification settings.
 * @returns The image repository consumed by the runtime stack.
 */
export function addProductionControl(
  stack: cdk.Stack,
  state: StateFoundations,
  settings: ProductionControlSettings,
): ProductionControl {
  addProductionTags(stack);
  const allowDestruction = settings.allowDestruction === true;
  const removalPolicy = allowDestruction ? cdk.RemovalPolicy.DESTROY : cdk.RemovalPolicy.RETAIN;

  const serverRepository = new ecr.Repository(stack, "ServerRepository", {
    emptyOnDelete: allowDestruction,
    imageScanOnPush: true,
    imageTagMutability: ecr.TagMutability.IMMUTABLE,
    lifecycleRules: [
      {
        description: "Keep the twenty newest immutable server images.",
        maxImageCount: 20,
        tagStatus: ecr.TagStatus.ANY,
      },
    ],
    removalPolicy,
    repositoryName: productionRepositoryName,
  });
  const serverLogGroup = new logs.LogGroup(stack, "ServerLogGroup", {
    logGroupName: productionLogGroupName,
    removalPolicy,
    retention: logs.RetentionDays.ONE_MONTH,
  });
  const githubOidcProvider = new iam.OidcProviderNative(stack, "GitHubOidcProvider", {
    clientIds: ["sts.amazonaws.com"],
    url: githubOidcProviderUrl,
  });
  const deploymentRole = new iam.Role(stack, "DeploymentRole", {
    assumedBy: new iam.WebIdentityPrincipal(githubOidcProvider.oidcProviderArn, {
      StringEquals: {
        "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
        "token.actions.githubusercontent.com:sub": githubDeploymentSubject,
      },
    }),
    description: "Deploys Cipher production only through the protected GitHub environment.",
    maxSessionDuration: cdk.Duration.hours(1),
    roleName: productionDeploymentRoleName,
  });
  const taskRole = new iam.Role(stack, "TaskRole", {
    assumedBy: new iam.ServicePrincipal("ecs-tasks.amazonaws.com"),
    description: "Cipher server data-plane role; starts with no AWS permissions.",
    roleName: productionTaskRoleName,
  });
  const executionRole = new iam.Role(stack, "ExecutionRole", {
    assumedBy: new iam.ServicePrincipal("ecs-tasks.amazonaws.com"),
    description: "Pulls the Cipher server image and writes its retained log stream.",
    roleName: productionExecutionRoleName,
  });
  const backupRole = new iam.Role(stack, "BackupRole", {
    assumedBy: new iam.ServicePrincipal("backup.amazonaws.com"),
    description: "Creates bounded Cipher DynamoDB recovery points through AWS Backup.",
    managedPolicies: [
      iam.ManagedPolicy.fromAwsManagedPolicyName(
        "service-role/AWSBackupServiceRolePolicyForBackup",
      ),
    ],
    roleName: productionBackupRoleName,
  });
  const backupVault = new backup.BackupVault(stack, "BackupVault", {
    backupVaultName: productionBackupVaultName,
    removalPolicy,
  });
  const backupPlan = backup.BackupPlan.daily35DayRetention(stack, "BackupPlan", backupVault);
  backupPlan.addSelection("PersistentTables", {
    backupSelectionName: "cipher-production-tables",
    disableDefaultBackupPolicy: true,
    resources: [
      backup.BackupResource.fromDynamoDbTable(state.usersTable),
      backup.BackupResource.fromDynamoDbTable(state.conversationsTable),
      backup.BackupResource.fromDynamoDbTable(state.messagesTable),
      backup.BackupResource.fromDynamoDbTable(state.mediaTable),
    ],
    role: backupRole,
  });

  addDeploymentPermissions(
    deploymentRole,
    state,
    backupRole,
    serverRepository,
    settings.stackNames,
  );
  addExecutionPermissions(executionRole, serverRepository, serverLogGroup);
  addBudget(stack, settings.budgetAlertEmail);

  addOutput(stack, "ServerRepositoryName", serverRepository.repositoryName);
  addOutput(stack, "ServerRepositoryUri", serverRepository.repositoryUri);
  addOutput(stack, "DeploymentRoleArn", deploymentRole.roleArn);
  addOutput(stack, "BackupRoleArn", backupRole.roleArn);
  addOutput(stack, "BackupVaultName", backupVault.backupVaultName);

  return {
    backupRole,
    backupVault,
    deploymentRole,
    executionRole,
    serverLogGroup,
    serverRepository,
    taskRole,
  };
}

/**
 * Grants the deployment role only the concrete image, stack-read, service-health, fixture, and recovery actions.
 *
 * @param deploymentRole - Web-identity role used by the deployment workflow.
 * @param state - Persistent resources that the workflow validates with owned fixtures.
 * @param backupRole - Service role passed only to AWS Backup.
 * @param serverRepository - Immutable application image repository.
 * @param stackNames - Exact Cipher stacks whose outputs the workflow may read.
 */
function addDeploymentPermissions(
  deploymentRole: iam.Role,
  state: StateFoundations,
  backupRole: iam.Role,
  serverRepository: ecr.Repository,
  stackNames: ProductionStackNames,
): void {
  serverRepository.grantPullPush(deploymentRole);
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["sts:AssumeRole"],
      resources: [
        cdkBootstrapRoleArn("deploy"),
        cdkBootstrapRoleArn("file-publishing"),
        cdkBootstrapRoleArn("image-publishing"),
        cdkBootstrapRoleArn("lookup"),
      ],
    }),
  );
  const stack = cdk.Stack.of(deploymentRole);
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["cloudformation:DescribeStacks"],
      resources: [cdkBootstrapStackName, ...Object.values(stackNames)].map((stackName) =>
        stack.formatArn({
          service: "cloudformation",
          resource: "stack",
          resourceName: `${stackName}/*`,
        }),
      ),
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["ecs:DescribeServices"],
      conditions: {
        ArnEquals: {
          "ecs:cluster": stack.formatArn({
            service: "ecs",
            resource: "cluster",
            resourceName: productionClusterName,
          }),
        },
      },
      resources: [
        stack.formatArn({
          service: "ecs",
          resource: "service",
          resourceName: `${productionClusterName}/${productionServiceName}`,
        }),
      ],
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: [
        "cognito-idp:AdminCreateUser",
        "cognito-idp:AdminDeleteUser",
        "cognito-idp:AdminGetUser",
      ],
      resources: [state.userPool.userPoolArn],
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["dynamodb:DeleteItem", "dynamodb:GetItem", "dynamodb:PutItem"],
      resources: [state.usersTable.tableArn],
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: [
        "s3:DeleteObject",
        "s3:GetObject",
        "s3:GetObjectTagging",
        "s3:PutObject",
        "s3:PutObjectTagging",
      ],
      resources: [state.mediaBucket.arnForObjects("fixtures/*")],
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["backup:DescribeBackupJob"],
      resources: ["*"],
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      // AWS Backup does not support resource-level authorization for StartBackupJob.
      // The workflow remains constrained to Cipher tables by its fixed command and backup role.
      actions: ["backup:StartBackupJob"],
      resources: ["*"],
    }),
  );
  deploymentRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["iam:PassRole"],
      conditions: { StringEquals: { "iam:PassedToService": "backup.amazonaws.com" } },
      resources: [backupRole.roleArn],
    }),
  );
}

/**
 * Grants the ECS execution role exactly the image-pull and log-stream permissions it uses.
 *
 * @param executionRole - ECS execution role for the one Cipher service.
 * @param serverRepository - Immutable application image repository.
 * @param serverLogGroup - Retained server log group owned with the execution role.
 */
function addExecutionPermissions(
  executionRole: iam.Role,
  serverRepository: ecr.Repository,
  serverLogGroup: logs.LogGroup,
): void {
  serverRepository.grantPull(executionRole);
  executionRole.addToPolicy(
    new iam.PolicyStatement({
      actions: ["logs:CreateLogStream", "logs:PutLogEvents"],
      resources: [serverLogGroup.logGroupArn],
    }),
  );
}

/**
 * Creates a tagged, account-level production cost budget with timely email notifications.
 *
 * @param stack - Stack that owns the production budget.
 * @param budgetAlertEmail - Validated address for actual and forecast threshold notifications.
 */
function addBudget(stack: cdk.Stack, budgetAlertEmail: string): void {
  new budgets.CfnBudget(stack, "ProductionBudget", {
    budget: {
      budgetLimit: { amount: 50, unit: "USD" },
      budgetName: productionBudgetName,
      budgetType: "COST",
      costFilters: { TagKeyValue: ["user:Application$cipher"] },
      timeUnit: "MONTHLY",
    },
    notificationsWithSubscribers: [
      {
        notification: {
          comparisonOperator: "GREATER_THAN",
          notificationType: "ACTUAL",
          threshold: 80,
          thresholdType: "PERCENTAGE",
        },
        subscribers: [{ address: budgetAlertEmail, subscriptionType: "EMAIL" }],
      },
      {
        notification: {
          comparisonOperator: "GREATER_THAN",
          notificationType: "FORECASTED",
          threshold: 100,
          thresholdType: "PERCENTAGE",
        },
        subscribers: [{ address: budgetAlertEmail, subscriptionType: "EMAIL" }],
      },
    ],
    resourceTags: [
      { key: "Application", value: "cipher" },
      { key: "Environment", value: "production" },
    ],
  });
}

/**
 * @param roleKind - Default CDK bootstrap role category.
 * @returns Exact default-bootstrap role ARN for one CDK capability.
 */
function cdkBootstrapRoleArn(roleKind: string): string {
  return cdk.Fn.join("", [
    "arn:",
    cdk.Aws.PARTITION,
    ":iam::",
    cdk.Aws.ACCOUNT_ID,
    `:role/cdk-${cdkBootstrapQualifier}-${roleKind}-role-`,
    cdk.Aws.ACCOUNT_ID,
    "-",
    cdk.Aws.REGION,
  ]);
}

/**
 * @param stack - Stack publishing the value.
 * @param id - Stable output identifier.
 * @param value - CloudFormation value for runtime image publishing.
 */
function addOutput(stack: cdk.Stack, id: string, value: string): void {
  new cdk.CfnOutput(stack, id, { value });
}
