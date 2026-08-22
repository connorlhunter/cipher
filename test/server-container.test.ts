import { describe, expect, test } from "bun:test";

import { rustVersionFromToolchain, serverImageBuildCommand } from "../scripts/build-server-image";
import { requiredToolchains } from "../scripts/toolchains";

const dockerfileUrl = new URL("../Dockerfile", import.meta.url);
const dockerignoreUrl = new URL("../.dockerignore", import.meta.url);
const rustToolchainUrl = new URL("../rust-toolchain.toml", import.meta.url);
const workspaceManifestUrl = new URL("../Cargo.toml", import.meta.url);

interface WorkspaceManifest {
  workspace?: {
    members?: unknown[];
  };
}

describe("server container", () => {
  test("builds only the release server binary and runs it without root", async () => {
    const [dockerfile, workspaceManifest] = await Promise.all([
      Bun.file(dockerfileUrl).text(),
      Bun.file(workspaceManifestUrl).text(),
    ]);
    const manifest = Bun.TOML.parse(workspaceManifest) as unknown as WorkspaceManifest;
    const members: unknown[] = Array.isArray(manifest.workspace?.members)
      ? manifest.workspace.members
      : [];

    expect(dockerfile).toContain(
      "ARG RUST_VERSION\nFROM rust:${RUST_VERSION}-slim-bookworm AS builder",
    );
    expect(dockerfile).not.toContain("1.96.0");
    expect(dockerfile).toContain("cargo build --locked --release --package cipher-server");
    expect(dockerfile).toContain("USER cipher:cipher");
    expect(dockerfile).toContain("ENV CIPHER_SERVER_BIND=0.0.0.0:3000");
    expect(dockerfile).toContain('ENTRYPOINT ["/usr/local/bin/cipher-server"]');

    expect(members.length).toBeGreaterThan(0);
    for (const member of members) {
      expect(typeof member).toBe("string");
      if (typeof member !== "string") {
        continue;
      }

      expect(dockerfile).toContain(`COPY ${member}/Cargo.toml ${member}/Cargo.toml`);
      expect(dockerfile).toContain(`COPY ${member} ${member}`);
    }

    const fetchIndex = dockerfile.indexOf("RUN cargo fetch --locked");
    expect(fetchIndex).toBeGreaterThan(dockerfile.indexOf("src-tauri/src/main.rs"));
  });

  test("keeps environment files and build outputs out of the build context", async () => {
    const dockerignore = await Bun.file(dockerignoreUrl).text();

    expect(dockerignore).toContain(".env*");
    expect(dockerignore).toContain("target");
  });

  test("derives the builder version from the canonical toolchain manifest", async () => {
    const toolchain = await Bun.file(rustToolchainUrl).text();
    const rustVersion = rustVersionFromToolchain(toolchain);

    expect(rustVersion).toBe(requiredToolchains.rust);
    expect(serverImageBuildCommand(rustVersion)).toEqual([
      "docker",
      "build",
      "--build-arg",
      `RUST_VERSION=${rustVersion}`,
      "--tag",
      "cipher-server",
      ".",
    ]);
  });

  test("requires a non-empty Rust toolchain channel", () => {
    expect(() => rustVersionFromToolchain("[toolchain]\nchannel = ''\n")).toThrow(
      "non-empty toolchain channel",
    );
  });
});
