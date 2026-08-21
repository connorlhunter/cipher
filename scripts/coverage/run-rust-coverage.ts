import { mkdirSync } from "node:fs";
import { join } from "node:path";

const outputPath = join("coverage", "rust.lcov");
mkdirSync("coverage", { recursive: true });

const result = Bun.spawnSync(
  [
    "cargo",
    "llvm-cov",
    "--workspace",
    "--locked",
    "--fail-under-lines",
    "95",
    "--fail-under-functions",
    "95",
    "--lcov",
    "--output-path",
    outputPath,
  ],
  { stderr: "inherit", stdout: "inherit" },
);

if (result.exitCode !== 0) {
  throw new Error("cargo-llvm-cov failed to generate Rust coverage.");
}
