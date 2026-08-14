import { readFileSync } from "node:fs";

/** Root manifest fields used to derive JavaScript tool requirements. */
interface PackageManifest {
  devDependencies?: Record<string, string>;
  packageManager?: string;
  toolchain?: {
    codeql?: string;
  };
}

/** Rust toolchain manifest fields used by repository scripts. */
interface RustToolchainManifest {
  toolchain?: {
    channel?: string;
  };
}

/**
 * @param value - Manifest value to validate.
 * @param label - Setting name used in an error.
 * @returns The required non-empty string.
 */
function requiredString(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${label} must be a non-empty string.`);
  }
  return value;
}

/**
 * @param packageManager - Package manager declaration from package.json.
 * @returns The pinned Bun version.
 */
function bunVersion(packageManager: string): string {
  const match = /^bun@(.+)$/u.exec(packageManager);
  if (match?.[1] === undefined) {
    throw new Error("package.json must pin Bun with packageManager.");
  }
  return match[1];
}

const packageManifest = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
) as PackageManifest;
const rustToolchainManifest = Bun.TOML.parse(
  readFileSync(new URL("../rust-toolchain.toml", import.meta.url), "utf8"),
) as unknown as RustToolchainManifest;

/** @description Toolchain requirements read from their canonical manifests. */
export const requiredToolchains = Object.freeze({
  bun: bunVersion(requiredString(packageManifest.packageManager, "packageManager")),
  codeql: requiredString(packageManifest.toolchain?.codeql, "toolchain.codeql"),
  rust: requiredString(rustToolchainManifest.toolchain?.channel, "Rust toolchain channel"),
  tauri: requiredString(
    packageManifest.devDependencies?.["@tauri-apps/cli"],
    "@tauri-apps/cli version",
  ),
});
