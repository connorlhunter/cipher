const repository = Bun.spawnSync(["git", "rev-parse", "--git-dir"], {
  stderr: "ignore",
  stdout: "ignore",
});

if (repository.exitCode === 0) {
  const configured = Bun.spawnSync(["git", "config", "core.hooksPath", ".githooks"], {
    stderr: "inherit",
    stdout: "inherit",
  });

  if (configured.exitCode !== 0) {
    throw new Error("Failed to configure the repository Git hooks.");
  }
}
