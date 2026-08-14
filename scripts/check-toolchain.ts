import { requiredToolchains } from "./toolchains";

/**
 * @param command - Version command to run.
 * @param args - Arguments passed to the command.
 * @returns The command's trimmed standard output.
 */
function commandVersion(command: string, args: string[]): string {
  const result = Bun.spawnSync([command, ...args], { stdout: "pipe", stderr: "pipe" });
  if (result.exitCode !== 0) {
    throw new Error(`${command} is required. Install the pinned toolchain and try again.`);
  }

  return new TextDecoder().decode(result.stdout).trim();
}

/**
 * @param output - Tool version output.
 * @param tool - Expected tool name.
 * @returns The semantic version portion of the output.
 */
function semanticVersion(output: string, tool: string): string {
  const version = output.split(" ")[1];
  if (version === undefined || !Bun.semver.satisfies(version, version)) {
    throw new Error(`Could not read the ${tool} version from: ${output}`);
  }
  return version;
}

if (!Bun.semver.satisfies(Bun.version, requiredToolchains.bun)) {
  throw new Error(`Cipher requires Bun ${requiredToolchains.bun}; found ${Bun.version}.`);
}

const rustVersion = commandVersion("rustc", ["--version"]);
if (semanticVersion(rustVersion, "Rust") !== requiredToolchains.rust) {
  throw new Error(`Cipher requires Rust ${requiredToolchains.rust}; found ${rustVersion}.`);
}

const cargoVersion = commandVersion("cargo", ["--version"]);
if (semanticVersion(cargoVersion, "Cargo") !== requiredToolchains.rust) {
  throw new Error(`Cipher requires Cargo ${requiredToolchains.rust}; found ${cargoVersion}.`);
}

if (process.platform !== "darwin" && process.platform !== "win32") {
  throw new Error("Cipher development is supported on macOS and Windows only.");
}

if (process.platform === "darwin" && process.arch !== "arm64") {
  throw new Error(`Cipher requires an Apple Silicon Mac; found ${process.arch}.`);
}

if (process.platform === "win32" && process.arch !== "x64") {
  throw new Error(`Cipher requires 64-bit Windows; found ${process.arch}.`);
}

if (process.platform === "darwin") {
  commandVersion("xcrun", ["--find", "clang"]);
}

const tauriVersion = commandVersion("bunx", ["tauri", "--version"]);
const installedTauri = semanticVersion(tauriVersion, "Tauri CLI");
if (!Bun.semver.satisfies(installedTauri, requiredToolchains.tauri)) {
  throw new Error(`Cipher requires Tauri CLI ${requiredToolchains.tauri}; found ${tauriVersion}.`);
}

console.log(`Bun ${Bun.version}`);
console.log(rustVersion);
console.log(cargoVersion);
console.log(tauriVersion);
