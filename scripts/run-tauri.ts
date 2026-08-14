const args = process.argv.slice(2);

if (args[0] === "build" && !args.includes("--bundles")) {
  args.push("--bundles", process.platform === "win32" ? "nsis" : "dmg");
}

const result = Bun.spawnSync(["bunx", "tauri", ...args], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    CARGO_TARGET_DIR: "target/tauri",
    RUSTUP_TOOLCHAIN: "1.96.0",
  },
  stderr: "inherit",
  stdout: "inherit",
});

process.exit(result.exitCode);
