import { describe, expect, test } from "bun:test";

import {
  createFixtureScope,
  liveRunner,
  loadLiveFixtureConfig,
  runLiveFixtureCheck,
  type CommandRunner,
} from "../scripts/live-fixtures";

const runId = "550e8400-e29b-41d4-a716-446655440000";
const fixtureEnvironment = {
  CIPHER_AWS_ACCOUNT_ID: "123456789012",
  CIPHER_AWS_REGION: "us-east-1",
  CIPHER_COGNITO_CLIENT_ID: "clientid",
  CIPHER_COGNITO_USER_POOL_ID: "us-east-1_example",
  CIPHER_CONVERSATIONS_TABLE: "cipher-production-conversations",
  CIPHER_MEDIA_BUCKET: "cipher-production-media-123456789012-us-east-1",
  CIPHER_MEDIA_TABLE: "cipher-production-media",
  CIPHER_MESSAGES_TABLE: "cipher-production-messages",
  CIPHER_STATE_STACK: "CipherProductionState",
  CIPHER_USERS_TABLE: "cipher-production-users",
};
const config = loadLiveFixtureConfig(fixtureEnvironment);

function response(command: ReadonlyArray<string>): string {
  if (command[1] === "sts") return "123456789012\n";
  if (command[1] === "cloudformation") {
    return JSON.stringify({
      Stacks: [
        {
          Outputs: [
            { OutputKey: "CognitoUserPoolId", OutputValue: config.cognitoUserPoolId },
            { OutputKey: "CognitoUserPoolClientId", OutputValue: config.cognitoClientId },
            { OutputKey: "UsersTableName", OutputValue: config.usersTable },
            { OutputKey: "ConversationsTableName", OutputValue: config.conversationsTable },
            { OutputKey: "MessagesTableName", OutputValue: config.messagesTable },
            { OutputKey: "MediaTableName", OutputValue: config.mediaTable },
            { OutputKey: "MediaBucketName", OutputValue: config.bucket },
          ],
        },
      ],
    });
  }
  if (command[1] === "cognito-idp" && command[2] === "admin-get-user") {
    return JSON.stringify({
      UserAttributes: [{ Name: "email", Value: command[command.indexOf("--username") + 1] }],
    });
  }
  if (command[1] === "dynamodb" && command[2] === "get-item") {
    const key = JSON.parse(command[command.indexOf("--key") + 1] ?? "{}") as {
      pk?: { S?: string };
    };
    return JSON.stringify({
      Item: key.pk?.S?.endsWith("-sentinel")
        ? { pk: key.pk, sk: key.pk }
        : { fixture_run_id: { S: runId }, pk: key.pk, sk: key.pk },
    });
  }
  if (command[1] === "s3api" && command[2] === "get-object-tagging") {
    return JSON.stringify({ TagSet: [{ Key: "fixture-run-id", Value: runId }] });
  }
  return "{}";
}

describe("live fixture scope", () => {
  test("limits resource names and object keys to one UUID v4 run", () => {
    const scope = createFixtureScope(runId);
    expect(scope.resourceId("alice")).toBe(`cipher-live-it-${runId}-alice`);
    expect(scope.objectKey("ciphertext")).toBe(`fixtures/${runId}/ciphertext`);
    expect(() => createFixtureScope("any")).toThrow("lowercase UUID v4");
    expect(() => scope.resourceId("alice/other")).toThrow("Fixture label");
  });

  test("rejects a non-production fixture target before AWS commands", () => {
    expect(() =>
      loadLiveFixtureConfig({
        CIPHER_AWS_ACCOUNT_ID: "123456789012",
        CIPHER_AWS_REGION: "us-west-2",
        CIPHER_COGNITO_CLIENT_ID: "clientid",
        CIPHER_COGNITO_USER_POOL_ID: "us-east-1_example",
        CIPHER_CONVERSATIONS_TABLE: "conversations",
        CIPHER_MEDIA_BUCKET: "bucket",
        CIPHER_MEDIA_TABLE: "media",
        CIPHER_MESSAGES_TABLE: "messages",
        CIPHER_STATE_STACK: "state",
        CIPHER_USERS_TABLE: "users",
      }),
    ).toThrow("must be us-east-1");

    expect(() =>
      loadLiveFixtureConfig({ ...fixtureEnvironment, CIPHER_AWS_ACCOUNT_ID: "000000000000" }),
    ).toThrow("12-digit production account");
    expect(() =>
      loadLiveFixtureConfig({ ...fixtureEnvironment, CIPHER_MEDIA_BUCKET: " " }),
    ).toThrow("CIPHER_MEDIA_BUCKET must be a non-empty value");
  });

  test("uses exact, marked cleanup and leaves unmarked sentinels outside it", () => {
    const commands: string[][] = [];
    const runner: CommandRunner = {
      run(command) {
        commands.push([...command]);
        return { exitCode: 0, stderr: "", stdout: response(command) };
      },
    };

    expect(runLiveFixtureCheck(config, runner, runId)).toHaveLength(3);
    const deleteCommands = commands.filter(
      (command) => command[2] === "delete-item" || command[2] === "delete-object",
    );
    expect(deleteCommands).toHaveLength(4);
    expect(
      deleteCommands.every(
        (command) => command.includes("--key") || command.includes("--condition-expression"),
      ),
    ).toBe(true);
    expect(deleteCommands.some((command) => command.join(" ").includes("-sentinel"))).toBe(true);
    const markedDelete = deleteCommands.find(
      (command) => command[2] === "delete-item" && command.includes("--condition-expression"),
    );
    expect(markedDelete).toContain("fixture_run_id = :run");
  });

  test("reports failed AWS commands without leaking configuration", () => {
    const runner: CommandRunner = {
      run() {
        return { exitCode: 1, stderr: "access denied", stdout: "" };
      },
    };

    expect(() => runLiveFixtureCheck(config, runner, runId)).toThrow(
      "AWS account check failed: access denied",
    );
  });

  test("uses the native fixture runner without a shell", () => {
    const result = liveRunner.run(["bun", "--version"]);
    expect(result.exitCode).toBe(0);
    expect(result.stdout.trim()).toMatch(/^1\./u);
  });
});
