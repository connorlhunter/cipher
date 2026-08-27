import { expect, test } from "bun:test";

import { defaultCommandRunner } from "../scripts/publish/command-runner";

test("completes a successful publication command", async () => {
  await defaultCommandRunner(
    process.execPath,
    ["-e", "process.stdout.write('published')"],
    "Artifact publication",
  );
});

test("reports command output when a publication command fails", async () => {
  await expect(
    defaultCommandRunner(
      process.execPath,
      ["-e", "process.stdout.write('details'); process.stderr.write('failure'); process.exit(2)"],
      "Artifact publication",
    ),
  ).rejects.toThrow("Artifact publication failed with exit code 2.\ndetails\nfailure");
});

test("reports a command that cannot be started", async () => {
  await expect(
    defaultCommandRunner("cipher-command-that-does-not-exist", [], "Artifact publication"),
  ).rejects.toThrow("Artifact publication failed");
});
