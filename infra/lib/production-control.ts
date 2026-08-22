/** Defines the retained image repository that supplies Cipher's production runtime. */
import * as cdk from "aws-cdk-lib";
import * as ecr from "aws-cdk-lib/aws-ecr";

const productionRepositoryName = "cipher-production-server";

/** Image resources retained while the runtime and network are paused. */
export interface ProductionControl {
  /** Dedicated immutable repository for the one Cipher server image. */
  readonly serverRepository: ecr.Repository;
}

/**
 * Adds the production repository before any runtime task is created.
 *
 * The repository belongs to the protected control stack so a pause can remove
 * hourly network and runtime resources without losing the image used to resume.
 *
 * @param stack - Cipher's protected production control stack.
 * @returns The image repository consumed by the runtime stack.
 */
export function addProductionControl(stack: cdk.Stack): ProductionControl {
  const serverRepository = new ecr.Repository(stack, "ServerRepository", {
    imageScanOnPush: true,
    imageTagMutability: ecr.TagMutability.IMMUTABLE,
    lifecycleRules: [
      {
        description: "Keep the twenty newest immutable server images.",
        maxImageCount: 20,
        tagStatus: ecr.TagStatus.ANY,
      },
    ],
    removalPolicy: cdk.RemovalPolicy.RETAIN,
    repositoryName: productionRepositoryName,
  });

  addOutput(stack, "ServerRepositoryName", serverRepository.repositoryName);
  addOutput(stack, "ServerRepositoryUri", serverRepository.repositoryUri);

  return { serverRepository };
}

/**
 * @param stack - Stack publishing the value.
 * @param id - Stable output identifier.
 * @param value - CloudFormation value for runtime image publishing.
 */
function addOutput(stack: cdk.Stack, id: string, value: string): void {
  new cdk.CfnOutput(stack, id, { value });
}
