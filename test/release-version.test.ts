import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { expect, test } from "bun:test";
import { synchronizeReleaseVersion } from "../scripts/release-version";

const packageVersion = (JSON.parse(readFileSync("package.json", "utf8")) as { version: string })
  .version;

test("keeps application release metadata aligned", () => {
  const cargoManifest = readFileSync("Cargo.toml", "utf8");
  const serverManifest = readFileSync("apps/cipher-server/Cargo.toml", "utf8");
  const desktopManifest = readFileSync("src-tauri/Cargo.toml", "utf8");
  const realtimeManifest = readFileSync("crates/cipher-realtime-protocol/Cargo.toml", "utf8");
  const nativeTransportManifest = readFileSync("crates/cipher-native-transport/Cargo.toml", "utf8");
  const cargoLock = readFileSync("Cargo.lock", "utf8");
  const tauriVersion = (
    JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")) as { version: string }
  ).version;

  expect(cargoManifest.match(/^version = "([^"]+)"/mu)?.[1]).toBe(packageVersion);
  for (const [manifest, dependency] of [
    [serverManifest, "cipher-types"],
    [desktopManifest, "cipher-desktop-lifecycle"],
    [desktopManifest, "cipher-native-transport"],
    [desktopManifest, "cipher-types"],
    [realtimeManifest, "cipher-types"],
    [nativeTransportManifest, "cipher-realtime-protocol"],
    [nativeTransportManifest, "cipher-types"],
  ] as const) {
    expect(
      manifest.match(new RegExp(`${dependency} = \\{[^\\n]+version = "\\^([^"]+)"`, "u"))?.[1],
    ).toBe(packageVersion);
  }
  expect(tauriVersion).toBe(packageVersion);

  for (const crate of [
    "cipher-desktop",
    "cipher-desktop-lifecycle",
    "cipher-native-transport",
    "cipher-realtime-protocol",
    "cipher-server",
    "cipher-test-support",
    "cipher-types",
  ]) {
    const lockedVersion = cargoLock.match(
      new RegExp(`\\[\\[package\\]\\]\\nname = "${crate}"\\nversion = "([^"]+)"`, "u"),
    )?.[1];
    expect(lockedVersion).toBe(packageVersion);
  }
});

test("synchronizes every release declaration from the package version", () => {
  const root = mkdtempSync("/tmp/cipher-release-version-");
  const originalDirectory = process.cwd();

  try {
    writeFixture(root, "package.json", '{"version":"1.2.3"}\n');
    writeFixture(root, "Cargo.toml", 'version = "0.0.0"\n');
    writeFixture(root, "src-tauri/tauri.conf.json", '{ "version": "0.0.0" }\n');
    writeFixture(
      root,
      "apps/cipher-server/Cargo.toml",
      'cipher-types = { path = "../../crates/cipher-types", version = "^0.0.0" }\n',
    );
    writeFixture(
      root,
      "src-tauri/Cargo.toml",
      [
        'cipher-desktop-lifecycle = { path = "../crates/cipher-desktop-lifecycle", version = "^0.0.0" }',
        'cipher-native-transport = { path = "../crates/cipher-native-transport", version = "^0.0.0" }',
        'cipher-types = { path = "../crates/cipher-types", version = "^0.0.0" }',
      ].join("\n"),
    );
    writeFixture(
      root,
      "crates/cipher-realtime-protocol/Cargo.toml",
      'cipher-types = { path = "../cipher-types", version = "^0.0.0" }\n',
    );
    writeFixture(
      root,
      "crates/cipher-native-transport/Cargo.toml",
      [
        'cipher-realtime-protocol = { path = "../cipher-realtime-protocol", version = "^0.0.0" }',
        'cipher-types = { path = "../cipher-types", version = "^0.0.0" }',
      ].join("\n"),
    );
    writeFixture(
      root,
      "Cargo.lock",
      [
        "cipher-desktop",
        "cipher-desktop-lifecycle",
        "cipher-native-transport",
        "cipher-realtime-protocol",
        "cipher-server",
        "cipher-test-support",
        "cipher-types",
      ]
        .map((name) => `[[package]]\nname = "${name}"\nversion = "0.0.0"`)
        .join("\n\n"),
    );

    process.chdir(root);
    synchronizeReleaseVersion();
    synchronizeReleaseVersion(true);

    expect(readFileSync("Cargo.toml", "utf8")).toContain('version = "1.2.3"');
    expect(readFileSync("src-tauri/tauri.conf.json", "utf8")).toContain('"version": "1.2.3"');
    expect(readFileSync("Cargo.lock", "utf8")).not.toContain('version = "0.0.0"');

    writeFileSync("Cargo.toml", 'version = "0.0.0"\n');
    expect(() => synchronizeReleaseVersion(true)).toThrow("Cargo.toml does not match package.json");
  } finally {
    process.chdir(originalDirectory);
    rmSync(root, { force: true, recursive: true });
  }
});

test("rejects a missing or invalid package release version", () => {
  const root = mkdtempSync("/tmp/cipher-release-version-");
  const originalDirectory = process.cwd();

  try {
    writeFixture(root, "package.json", "{}\n");
    process.chdir(root);
    expect(() => synchronizeReleaseVersion()).toThrow(
      "package.json must contain a valid release version",
    );
  } finally {
    process.chdir(originalDirectory);
    rmSync(root, { force: true, recursive: true });
  }
});

function writeFixture(root: string, path: string, contents: string): void {
  const target = join(root, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, contents);
}
