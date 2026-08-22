# Production ingress

Cipher has one AWS environment in `us-east-1`. This document fixes the
ingress and task-networking choices that `CipherProductionNetwork` and
`CipherProductionRuntime` must implement. It intentionally does not create a
development, preview, or staging environment.

The executable baseline is
[`infra/lib/production-ingress.ts`](../infra/lib/production-ingress.ts). Its
tests keep the hostname, listener, task port, WebSocket path, and no-NAT
decision from drifting before the CDK stacks are added.

## Shape

- AWS CDK is the only infrastructure tool. Production changes go through a
  reviewed CloudFormation plan; neither the AWS console nor a second stack
  environment is part of the workflow.
- One internet-facing Application Load Balancer spans two public subnets in
  two Availability Zones. It has exactly one `HTTPS:443` listener using the
  existing `*.connorhunter.me` ACM certificate in `us-east-1`.
- Route 53 publishes an `A` alias for `cipher.connorhunter.me` in the existing
  `connorhunter.me` hosted zone. There is no HTTP listener or redirect route.
- One target group forwards HTTP/1.1 to port `3000` of the single modular Rust
  task. Its health check is `GET /healthz`; `/v1/realtime` uses the same target
  group and HTTP-to-WebSocket upgrade path.
- The task runs in the public subnets with a public IP so it can reach AWS APIs
  without a NAT gateway. Its security group permits inbound TCP `3000` only
  from the load-balancer security group. A public IP is therefore not a public
  entry point. The runtime and deployment identity work completes the least-
  privilege egress rules needed for AWS service APIs and image/log delivery.
- The ALB security group permits public inbound TCP `443` only. TLS terminates
  at the ALB; the ALB-to-task hop is the private VPC hop. The load balancer
  preserves the native `Authorization` request header during the WebSocket
  upgrade. The backend must validate it before accepting the realtime session.

No NAT gateway is approved for this environment. Adding one, moving the task
to private subnets, or adding interface endpoints changes the cost and network
decision and needs an explicit follow-up approval.

## Idle-cost estimate

The estimate uses 30 days (720 hours), one Linux/x86 Fargate task with `0.25`
vCPU and `0.5` GiB memory, and current published `us-east-1` rates as of
2026-08-22.

| Item                      | Calculation                       | Monthly estimate |
| ------------------------- | --------------------------------- | ---------------: |
| Application Load Balancer | `$0.0225 × 720`                   |         `$16.20` |
| One Fargate task          | CPU plus memory                   |          `$8.89` |
| Public IPv4 addresses     | Three addresses at `$0.005 × 720` |         `$10.80` |
| Baseline                  | Before traffic-driven charges     |         `$35.89` |

The baseline excludes ALB LCUs, data transfer, ECR storage, CloudWatch logs,
Cognito, DynamoDB, S3, Route 53, and taxes. It includes two ALB public IPv4
addresses and one task public IPv4 address. A NAT gateway would add at least
its hourly and data-processing charges; it is deliberately absent. Recheck
the [Application Load Balancer](https://aws.amazon.com/elasticloadbalancing/pricing/),
[Fargate](https://aws.amazon.com/ecs/pricing/), and
[VPC](https://aws.amazon.com/vpc/pricing/) price pages before deployment.

## Native live verification

Normal automated tests do not contact AWS. After the network and runtime
stacks have deployed, run this opt-in native Rust check using an access token
for an invited fixture account:

```sh
CIPHER_INGRESS_ACCESS_TOKEN='…' \
  cargo test -p cipher-native-transport production_ingress_serves_health_and_accepts_a_native_authorized_upgrade -- --ignored
```

The check requires an HTTPS `200` from `/healthz`, opens
`wss://cipher.connorhunter.me/v1/realtime`, and puts the access token only in
the native WebSocket `Authorization: Bearer` upgrade header. It does not print
the token. Record the successful fixture run on the ingress issue before
closing it; a failure leaves the issue open and must not be worked around with
an alternate hostname or environment.
