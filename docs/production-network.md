# Production network

Cipher has one AWS network: `CipherProductionNetwork` in `us-east-1`. It is
not a template for development, preview, or staging environments. The stack
uses the fixed ingress contract in [production ingress](./production-ingress.md)
and is consumed by the later production runtime stack.

## Shape

- One IPv4 VPC named `cipher-production-network` (`10.72.0.0/16`).
- Two `/24` public subnets in `us-east-1a` and `us-east-1b`. The fixed
  production names keep CDK synthesis AWS-free in local checks and continuous
  integration.
- One internet gateway and one default route per public subnet.
- No private or isolated subnets, NAT gateways, Elastic IP addresses, or VPC
  endpoints. The closed alpha deliberately avoids the recurring NAT cost.
- A future task receives a public IP for outbound AWS access, but it has no
  direct public ingress rule.

The runtime stack will attach the `cipher-production-ingress` security group to
its internet-facing Application Load Balancer. That group accepts only IPv4 TCP
443 and can send only TCP 3000 to `cipher-production-service`. The service
group accepts TCP 3000 only from the ingress group. Its outbound rules are
limited to TCP 443 and TCP/UDP 53 for AWS API access and DNS resolution.

There is no HTTP listener, direct task port, database, media bucket, or
administrative endpoint exposed by this stack. TLS termination, the Route 53
alias, health checks, and the authenticated WebSocket route belong to the
runtime deployment.

## Ownership, cost, and deletion controls

The VPC, subnet, route-table, internet-gateway, and security-group resources
receive these allocation tags:

| Tag           | Value               |
| ------------- | ------------------- |
| `Application` | `cipher`            |
| `Environment` | `production`        |
| `CostCenter`  | `cipher-production` |
| `ManagedBy`   | `cdk`               |

Activate the first three as AWS cost-allocation tags before relying on Cost
Explorer grouping. The network is intentionally removable during a closed-alpha
pause, so its resources do not use retain policies. The safety boundary is the
production control command: it verifies the configured 12-digit AWS account,
requires an interactive terminal and action-specific confirmation, and targets
only the four exact Cipher stack names. It never discovers stacks by prefix or
wildcard.

## Production changes

Production changes start with a read-only readiness check, followed by a CDK
deployment plan and an explicit terminal approval. The resume control runs CDK
with `--require-approval any-change`; it displays the synthesized change set and
does not apply it until the operator confirms it.

```sh
bun --env-file=.env run infra:readiness
bun --env-file=.env run infra:resume -- \
  --confirm=RESUME-CIPHER-PRODUCTION-123456789012-us-east-1
```

Use the account ID from the deployment configuration, not the example above.
The subsequent deployment workflow adds its own protected approval boundary,
recovery point, smoke test, and cleanup; no console-created network resources
are part of this design.
