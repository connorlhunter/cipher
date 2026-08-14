/** Runs Tauri with Cipher's isolated build output and platform bundle default. */
const args = process.argv.slice(2);
const environment = {
  ...process.env,
  CARGO_TARGET_DIR: "target/tauri",
  // Avoid macOS proc-macro linker corruption on fresh release builds.
  ...(process.platform === "darwin" ? { CARGO_BUILD_JOBS: "1" } : {}),
};

if (args[0] === "build" && !args.includes("--bundles")) {
  args.push("--bundles", process.platform === "win32" ? "nsis" : "dmg");
}

const result = Bun.spawnSync(["bunx", "tauri", ...args], {
  cwd: process.cwd(),
  env: environment,
  stderr: "inherit",
  stdout: "inherit",
});

process.exit(result.exitCode);
