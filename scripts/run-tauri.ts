/** Runs Tauri with Cipher's isolated build output and platform bundle default. */
import { resolve } from "node:path";

const args = process.argv.slice(2);
const environment = {
  ...process.env,
  // Tauri invokes Cargo from `src-tauri`; use an absolute path so Cargo keeps
  // one cache instead of rebasing a relative target directory per invocation.
  CARGO_TARGET_DIR: resolve(process.cwd(), "target", "tauri"),
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
