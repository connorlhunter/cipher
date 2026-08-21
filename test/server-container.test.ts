import { describe, expect, test } from "bun:test";

import { rustVersionFromToolchain, serverImageBuildCommand } from "../scripts/build-server-image";
import { requiredToolchains } from "../scripts/toolchains";

const dockerfileUrl = new URL("../Dockerfile", import.meta.url);
const dockerignoreUrl = new URL("../.dockerignore", import.meta.url);
const rustToolchainUrl = new URL("../rust-toolchain.toml", import.meta.url);

describe("server container", () => {
  test("builds only the release server binary and runs it without root", async () => {
    const dockerfile = await Bun.file(dockerfileUrl).text();

    expect(dockerfile).toContain(
      "ARG RUST_VERSION\nFROM rust:${RUST_VERSION}-slim-bookworm AS builder",
    );
    expect(dockerfile).not.toContain("1.96.0");
    expect(dockerfile).toContain("cargo build --locked --release --package cipher-server");
    expect(dockerfile).toContain("USER cipher:cipher");
    expect(dockerfile).toContain("ENV CIPHER_SERVER_BIND=0.0.0.0:3000");
    expect(dockerfile).toContain('ENTRYPOINT ["/usr/local/bin/cipher-server"]');
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
