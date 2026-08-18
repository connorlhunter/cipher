import { readFileSync } from "node:fs";

/** Rust toolchain manifest fields used to build the server image. */
interface RustToolchainManifest {
  toolchain?: {
    channel?: unknown;
  };
}

/**
 * Extracts the Rust version used for the server image from its toolchain manifest.
 *
 * @param contents - Contents of rust-toolchain.toml.
 * @returns The required Rust toolchain channel.
 */
export function rustVersionFromToolchain(contents: string): string {
  const manifest = Bun.TOML.parse(contents) as unknown as RustToolchainManifest;
  const rustVersion = manifest.toolchain?.channel;
  if (typeof rustVersion !== "string" || rustVersion.length === 0) {
    throw new Error("rust-toolchain.toml must define a non-empty toolchain channel.");
  }
  return rustVersion;
}

/**
 * Builds the literal Docker command for the pinned server image.
 *
 * @param rustVersion - Rust version read from rust-toolchain.toml.
 * @returns The Docker build command and arguments.
 */
export function serverImageBuildCommand(rustVersion: string): string[] {
  return [
    "docker",
    "build",
    "--build-arg",
    `RUST_VERSION=${rustVersion}`,
    "--tag",
    "cipher-server",
    ".",
  ];
}

if (import.meta.main) {
  const rustToolchain = readFileSync(new URL("../rust-toolchain.toml", import.meta.url), "utf8");
  const command = serverImageBuildCommand(rustVersionFromToolchain(rustToolchain));
  const result = Bun.spawnSync(command, { stderr: "inherit", stdout: "inherit" });

  process.exit(result.exitCode);
}
