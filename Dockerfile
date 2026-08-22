# syntax=docker/dockerfile:1

ARG RUST_VERSION
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY apps/cipher-server/Cargo.toml apps/cipher-server/Cargo.toml
COPY crates/cipher-desktop-lifecycle/Cargo.toml crates/cipher-desktop-lifecycle/Cargo.toml
COPY crates/cipher-native-transport/Cargo.toml crates/cipher-native-transport/Cargo.toml
COPY crates/cipher-realtime-protocol/Cargo.toml crates/cipher-realtime-protocol/Cargo.toml
COPY crates/cipher-test-support/Cargo.toml crates/cipher-test-support/Cargo.toml
COPY crates/cipher-types/Cargo.toml crates/cipher-types/Cargo.toml
COPY src-tauri/Cargo.toml src-tauri/Cargo.toml

# Cargo validates every workspace member before it fetches dependencies. These
# placeholders keep that validation valid while preserving a dependency-only
# cache layer; the real sources below replace them before the release build.
RUN mkdir -p apps/cipher-server/src \
    crates/cipher-desktop-lifecycle/src \
    crates/cipher-native-transport/src \
    crates/cipher-realtime-protocol/src \
    crates/cipher-test-support/src \
    crates/cipher-types/src \
    src-tauri/src \
    && touch apps/cipher-server/src/lib.rs \
    crates/cipher-desktop-lifecycle/src/lib.rs \
    crates/cipher-native-transport/src/lib.rs \
    crates/cipher-realtime-protocol/src/lib.rs \
    crates/cipher-test-support/src/lib.rs \
    crates/cipher-types/src/lib.rs \
    src-tauri/src/main.rs

RUN cargo fetch --locked

COPY apps/cipher-server apps/cipher-server
COPY crates/cipher-desktop-lifecycle crates/cipher-desktop-lifecycle
COPY crates/cipher-native-transport crates/cipher-native-transport
COPY crates/cipher-realtime-protocol crates/cipher-realtime-protocol
COPY crates/cipher-test-support crates/cipher-test-support
COPY crates/cipher-types crates/cipher-types
COPY src-tauri src-tauri

RUN cargo build --locked --release --package cipher-server

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates \
    && groupadd --system --gid 10001 cipher \
    && useradd --system --uid 10001 --gid cipher --home-dir /nonexistent --shell /usr/sbin/nologin cipher \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder --chown=cipher:cipher /workspace/target/release/cipher-server /usr/local/bin/cipher-server

USER cipher:cipher

EXPOSE 3000
ENV CIPHER_SERVER_BIND=0.0.0.0:3000

ENTRYPOINT ["/usr/local/bin/cipher-server"]
