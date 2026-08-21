import { readFileSync } from "node:fs";

import { expect, test } from "bun:test";

const packageVersion = (JSON.parse(readFileSync("package.json", "utf8")) as { version: string })
  .version;

test("keeps application release metadata aligned", () => {
  const cargoManifest = readFileSync("Cargo.toml", "utf8");
  const serverManifest = readFileSync("apps/cipher-server/Cargo.toml", "utf8");
  const cargoLock = readFileSync("Cargo.lock", "utf8");
  const tauriVersion = (
    JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")) as { version: string }
  ).version;

  expect(cargoManifest.match(/^version = "([^"]+)"/mu)?.[1]).toBe(packageVersion);
  expect(serverManifest.match(/cipher-types = \{[^\n]+version = "\^([^"]+)"/u)?.[1]).toBe(
    packageVersion,
  );
  expect(tauriVersion).toBe(packageVersion);

  for (const crate of [
    "cipher-desktop",
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
