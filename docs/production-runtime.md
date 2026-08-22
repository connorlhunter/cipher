# Production runtime

`CipherProductionControl` retains the `cipher-production-server` ECR repository. It scans every pushed image, rejects tag overwrites, retains the twenty newest images, and remains in place while the runtime is paused.

`CipherProductionRuntime` contains one internet-facing Application Load Balancer and one Fargate service. It is deliberately separate from state and control so `infra:pause` can remove the hourly runtime and network resources without deleting identities, data, images, or retained logs.

## Required deployment configuration

Set these values in the ignored production `.env` file before a deployment:

- `CIPHER_ACM_CERTIFICATE_ARN`: the issued `cipher.connorhunter.me` certificate in `us-east-1` emitted by `infra:certificate`.
- `CIPHER_HOSTED_ZONE_ID`: the existing `connorhunter.me` Route 53 hosted-zone ID.

The hosted zone and certificate remain outside Cipher's four CloudFormation stacks. CDK imports their exact identifiers; it does not create a parallel zone or certificate. The guarded certificate command creates the one DNS-validated certificate without a console step and refuses a conflicting validation record.

## Image and deployment sequence

1. Deploy `CipherProductionControl` to create the retained ECR repository.
2. Build the server image and push one immutable tag, normally the full source commit ID.
3. Deploy `CipherProductionRuntime` with `ServerImageTag` set to that pushed tag.
4. Verify `https://cipher.connorhunter.me/healthz` and the native WebSocket upgrade.

The service has exactly one desired task, a deployment circuit breaker with rollback, `minimumHealthyPercent` of zero, and `maximumPercent` of 100. Those settings require the old gateway task to stop before the replacement starts, so two independent gateway routers never run at once. The target group waits 60 seconds before deregistration, and the load balancer retains idle WebSocket connections for up to five minutes while the replacement drains.

Use a new immutable image tag for a forward fix. The retained image history is the recovery path until shared gateway routing exists; clients reconnect through the same `wss://cipher.connorhunter.me/v1/realtime` endpoint.

## Runtime shape

- HTTPS only on TCP 443, using the issued exact-host certificate.
- One `A` alias at `cipher.connorhunter.me`.
- HTTP target traffic only from the load balancer to port 3000.
- `/healthz` checks every 30 seconds and requires HTTP 200.
- One 0.25 vCPU / 0.5 GiB Fargate task in public subnets, with public addressing protected by the task security group.
- CloudWatch log group `/cipher/production/server`, retained for 30 days in the protected Control stack.

The task receives resource identifiers from the protected State stack. They are configuration values rather than image contents: the server image never includes an `.env` file, production account identifier, endpoint, or resource name. When a server feature genuinely requires a secret, its complete Secrets Manager ARN can be supplied as `CIPHER_RUNTIME_SECRET_ARN`; ECS resolves it through the execution role rather than writing plaintext into source, templates, or the image.
