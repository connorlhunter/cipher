const requiredBun = "1.3.14";
const requiredRust = "rustc 1.96.0";
const requiredTauri = "tauri-cli 2.11.4";

function commandVersion(command: string, args: string[]): string {
  const result = Bun.spawnSync([command, ...args], { stdout: "pipe", stderr: "pipe" });
  if (result.exitCode !== 0) {
    throw new Error(`${command} is required. Install the pinned toolchain and try again.`);
  }

  return new TextDecoder().decode(result.stdout).trim();
}

if (Bun.version !== requiredBun) {
  throw new Error(`Cipher requires Bun ${requiredBun}; found ${Bun.version}.`);
}

const rustVersion = commandVersion("rustc", ["--version"]);
if (!rustVersion.startsWith(requiredRust)) {
  throw new Error(`Cipher requires ${requiredRust}; found ${rustVersion}.`);
}

const cargoVersion = commandVersion("cargo", ["--version"]);
if (!cargoVersion.startsWith("cargo 1.96.0")) {
  throw new Error(`Cipher requires cargo 1.96.0; found ${cargoVersion}.`);
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
if (tauriVersion !== requiredTauri) {
  throw new Error(`Cipher requires ${requiredTauri}; found ${tauriVersion}.`);
}

console.log(`Bun ${Bun.version}`);
console.log(rustVersion);
console.log(cargoVersion);
console.log(tauriVersion);
