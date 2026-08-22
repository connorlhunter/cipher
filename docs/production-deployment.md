# Production deployment controls

Cipher has one production account and four exact CloudFormation stacks:

- `CipherProductionState` retains Cognito, DynamoDB, and private media.
- `CipherProductionControl` retains immutable images, deployment access, logs, backups, and cost controls.
- `CipherProductionNetwork` and `CipherProductionRuntime` are the disposable runtime boundary.

The deployment workflow runs only from `main` through GitHub's protected `production` environment. It exchanges a GitHub OpenID Connect token for the scoped `cipher-production-deployment` role; no long-lived AWS key is stored in the repository or deployment environment. The IAM trust policy accepts only `repo:connorlhunter/cipher:environment:production` tokens. The role can read only these stacks and the protected CDK bootstrap stack, and it can wait only for Cipher's one ECS service. AWS Backup's `StartBackupJob` action has no resource-level IAM authorization, so the workflow fixes its DynamoDB resource list and can pass only Cipher's dedicated backup role. Configure that GitHub environment with a required reviewer and a `main` branch rule before enabling dispatches.

## One-time bootstrap

Run these commands from an interactive terminal authenticated to the configured production account. They never discover resources by a prefix or wildcard.

1. Provision the public certificate outside the four Cipher stacks. This keeps it available during a runtime pause and a full stack teardown.

   ```sh
   bun --env-file=.env run infra:certificate -- \
     --confirm=PROVISION-CIPHER-PRODUCTION-CERTIFICATE-<account-id>-us-east-1
   ```

   The command refuses an account mismatch, requires exactly one `connorhunter.me.` hosted zone, and only creates the ACM DNS-validation CNAME when no conflicting record exists. Persist the two emitted configuration lines in the ignored production `.env` file after ACM reports the certificate as issued.

2. Set `CIPHER_BUDGET_ALERT_EMAIL` to a monitored inbox. Activate the `Application` user-defined cost-allocation tag before relying on the tag-filtered budget:

   ```sh
   aws ce update-cost-allocation-tags-status \
     --cost-allocation-tags-status TagKey=Application,Status=Active
   ```

3. Run the read-only preflight, review the plan, and create only the protected State and Control stacks. This establishes the image repository, backup vault, and OpenID Connect deployment role without trying to launch a bootstrap-tag runtime image.

   ```sh
   bun --env-file=.env run infra:readiness
   npm --prefix infra exec cdk -- \
     --app "npm --prefix infra exec tsx -- infra/bin/cipher.ts" \
     diff \
     "$CIPHER_STATE_STACK" "$CIPHER_CONTROL_STACK"
   npm --prefix infra exec cdk -- \
     --app "npm --prefix infra exec tsx -- infra/bin/cipher.ts" \
     deploy \
     "$CIPHER_STATE_STACK" "$CIPHER_CONTROL_STACK" \
     --require-approval any-change
   ```

4. Set the protected GitHub environment variables with the exact values from `.env`. They are identifiers, not secrets:

   ```sh
   gh variable set CIPHER_AWS_ACCOUNT_ID --env production --body "$CIPHER_AWS_ACCOUNT_ID"
   gh variable set CIPHER_AWS_REGION --env production --body "$CIPHER_AWS_REGION"
   gh variable set CIPHER_ACM_CERTIFICATE_ARN --env production --body "$CIPHER_ACM_CERTIFICATE_ARN"
   gh variable set CIPHER_HOSTED_ZONE_ID --env production --body "$CIPHER_HOSTED_ZONE_ID"
   gh variable set CIPHER_BUDGET_ALERT_EMAIL --env production --body "$CIPHER_BUDGET_ALERT_EMAIL"
   gh variable set CIPHER_STATE_STACK --env production --body "$CIPHER_STATE_STACK"
   gh variable set CIPHER_CONTROL_STACK --env production --body "$CIPHER_CONTROL_STACK"
   gh variable set CIPHER_NETWORK_STACK --env production --body "$CIPHER_NETWORK_STACK"
   gh variable set CIPHER_RUNTIME_STACK --env production --body "$CIPHER_RUNTIME_STACK"
   ```

   Set `CIPHER_RUNTIME_SECRET_ARN` only when the server genuinely needs a secret. ECS reads that optional Secrets Manager value through the execution role at launch; its plaintext is never emitted into a template, image, or workflow log.

## Routine deployment

Dispatch **Deploy production** from `main`. The plan job is protected by the `production` environment and records the CDK diff in the workflow summary. After review and approval, the deployment job receives a fresh one-hour OpenID Connect session and:

1. runs the production readiness preflight;
2. builds a server image and pushes a unique immutable tag to the retained repository;
3. creates four AWS Backup recovery points with 35-day retention before applying changes;
4. deploys the reviewed State, Control, Network, and Runtime stacks with that image tag;
5. waits for the one ECS service, checks `https://cipher.connorhunter.me/healthz`, and runs the native WSS smoke check;
6. creates and removes only UUID-owned Cognito, DynamoDB, and S3 fixtures.

The normal path is forward-fix: deploy a new immutable image tag. To roll back a bad runtime release without another environment, dispatch the workflow with a previously retained image tag. The Fargate service is explicitly stop-before-start (`minimumHealthyPercent: 0`, `maximumPercent: 100`) and the target group drains existing connections for 60 seconds.

## Recovery, retention, and cost limits

- DynamoDB uses AWS-managed encryption, 35-day point-in-time recovery, on-demand billing, and deletion protection.
- AWS Backup keeps a daily 35-day table backup plan in a vault that uses the AWS-managed key. Each deployment also creates bounded recovery points before it can change the runtime.
- Private media uses SSE-S3, versioning, and a 35-day noncurrent-version lifecycle. Pending and fixture ciphertext remain aggressively short-lived.
- The monthly production budget tracks `user:Application$cipher`, alerts the configured mailbox at 80% actual spend and 100% forecast spend, and resources carry `Application`, `Environment`, `CostCenter`, and `ManagedBy` tags.
- The ECS task role has no data-plane permissions until a concrete server feature needs one. The execution role can pull only Cipher's image, write only the retained server log group, and read the configured optional runtime secret.

## Lifecycle controls

Pause removes only `CipherProductionRuntime` and `CipherProductionNetwork`.
State, private media, the image repository, deployment access, backups, and
retained logs stay in place. The command verifies the configured account and
requires an interactive terminal:

```sh
bun --env-file=.env run infra:pause -- \
  --confirm=PAUSE-CIPHER-PRODUCTION-<account-id>-us-east-1
```

Resume runs the same read-only readiness check again, then plans and restores
all four stacks using one exact immutable image tag that already exists in the
retained repository:

```sh
bun --env-file=.env run infra:resume -- \
  --image-tag=<retained-immutable-server-image-tag> \
  --confirm=RESUME-CIPHER-PRODUCTION-<account-id>-us-east-1
```

Both actions are repeatable. A paused runtime has no Fargate, load-balancer, or
public-network hourly charge, while the retained state and control resources
can still incur their documented storage costs.

`infra:destroy-all` is reserved for the first empty-deployment drill before
alpha data exists. Start with its non-mutating plan, which names only the four
Cipher stacks:

```sh
bun --env-file=.env run infra:destroy-all -- \
  --confirm=UNLOCK-CIPHER-PRODUCTION-<account-id>-us-east-1 \
  --destroy-confirm=DESTROY-CIPHER-PRODUCTION-AND-ALL-DATA-<account-id>-us-east-1 \
  --dry-run
```

Running the same command without `--dry-run` requires both confirmations and
an interactive terminal in the configured production account. It first updates
only State and Control with the destructive CDK context. That temporarily
disables their deletion protection and switches their retained resources to
deletion mode. It then removes the disposable Runtime and Network stacks,
deletes recovery points returned by the exact
`cipher-production-recovery` vault, and destroys Control before State. The
destructive State synthesis empties the versioned media bucket; the destructive
Control synthesis empties the retained ECR repository. The Route 53 hosted
zone, ACM certificate, and CDK bootstrap remain outside this command.

The command never claims instant physical erasure: AWS can retain
provider-managed recovery material for a limited period, including Cognito and
DynamoDB recovery data. Record the destruction receipt and verify the four
named stacks are gone before closing the production account.
