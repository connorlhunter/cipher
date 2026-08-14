import { requiredToolchains } from "./toolchains";

const cargoDenyVersion = "0.20.2";

/**
 * @param command - Executable and arguments to run.
 * @returns Nothing; throws when the command fails.
 */
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
  requiredToolchains.rust,
  "--profile",
  "minimal",
  "--component",
  "clippy,rustfmt",
]);
const cargoDeny = Bun.spawnSync(["cargo", "deny", "--version"], {
  stderr: "ignore",
  stdout: "pipe",
});
const installedCargoDeny = new TextDecoder().decode(cargoDeny.stdout).trim();
if (cargoDeny.exitCode !== 0 || !installedCargoDeny.endsWith(` ${cargoDenyVersion}`)) {
  run(["cargo", "install", "cargo-deny", "--version", cargoDenyVersion, "--locked"]);
}
run(["bun", "install", "--frozen-lockfile"]);
run(["bun", "run", "toolchain:check"]);
