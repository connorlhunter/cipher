import { readFileSync } from "node:fs";

import { expect, test } from "bun:test";

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
