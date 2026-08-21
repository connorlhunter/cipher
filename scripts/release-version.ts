import { readFileSync, writeFileSync } from "node:fs";

const packageJsonPath = "package.json";
const cargoTomlPath = "Cargo.toml";
const cargoLockPath = "Cargo.lock";
const tauriConfigPath = "src-tauri/tauri.conf.json";
const workspaceCrates = [
  "cipher-desktop",
  "cipher-realtime-protocol",
  "cipher-server",
  "cipher-test-support",
  "cipher-types",
];
const versionedPathDependencies = [
  {
    matcher: /(cipher-types = \{ path = "[^"]+", version = ")\^[^"]+(" \})/u,
    path: "apps/cipher-server/Cargo.toml",
  },
] as const;

/** Reads the canonical application release version from package.json. */
function packageVersion(): string {
  const value = JSON.parse(readFileSync(packageJsonPath, "utf8")) as { version?: unknown };

  if (typeof value.version !== "string" || !/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/u.test(value.version)) {
    throw new Error("package.json must contain a valid release version.");
  }

  return value.version;
}

/** Replaces one declared release version or reports that it has drifted. */
function synchronizeVersion(
  path: string,
  matcher: RegExp,
  replacement: string,
  checkOnly: boolean,
): void {
  const current = readFileSync(path, "utf8");
  if (!matcher.test(current)) {
    throw new Error(`Could not find the release version in ${path}.`);
  }
  const next = current.replace(matcher, replacement);

  if (checkOnly && next !== current) {
    throw new Error(`${path} does not match package.json. Run bun run version:sync.`);
  }
  if (!checkOnly && next !== current) writeFileSync(path, next);
}

/** Synchronizes release metadata from package.json or verifies that it is aligned. */
export function synchronizeReleaseVersion(checkOnly = false): void {
  const version = packageVersion();
  synchronizeVersion(cargoTomlPath, /^version = "[^"]+"/mu, `version = "${version}"`, checkOnly);
  synchronizeVersion(tauriConfigPath, /"version": "[^"]+"/u, `"version": "${version}"`, checkOnly);
  for (const dependency of versionedPathDependencies) {
    synchronizeVersion(dependency.path, dependency.matcher, `$1^${version}$2`, checkOnly);
  }
  for (const crate of workspaceCrates) {
    synchronizeVersion(
      cargoLockPath,
      new RegExp(`(name = "${crate}"\\nversion = ")[^"]+(")`, "u"),
      `$1${version}$2`,
      checkOnly,
    );
  }
}

if (import.meta.main) {
  synchronizeReleaseVersion(process.argv.includes("--check"));
}
