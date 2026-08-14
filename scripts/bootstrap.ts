function run(command: string[]): void {
  const result = Bun.spawnSync(command, {
    stderr: "inherit",
    stdout: "inherit",
  });

  if (result.exitCode !== 0) {
    throw new Error(`Command failed: ${command[0]}`);
  }
}

run([
  "rustup",
  "toolchain",
  "install",
  "1.96.0",
  "--profile",
  "minimal",
  "--component",
  "clippy,rustfmt",
]);
const cargoDeny = Bun.spawnSync(["cargo", "deny", "--version"], {
  stderr: "ignore",
  stdout: "pipe",
});
const cargoDenyVersion = new TextDecoder().decode(cargoDeny.stdout).trim();
if (cargoDeny.exitCode !== 0 || !cargoDenyVersion.endsWith(" 0.20.2")) {
  run(["cargo", "install", "cargo-deny", "--version", "0.20.2", "--locked"]);
}
run(["bun", "install", "--frozen-lockfile"]);
run(["bun", "run", "toolchain:check"]);
