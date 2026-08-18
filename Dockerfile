# syntax=docker/dockerfile:1

ARG RUST_VERSION
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY apps/cipher-server/Cargo.toml apps/cipher-server/Cargo.toml
COPY crates/cipher-test-support/Cargo.toml crates/cipher-test-support/Cargo.toml
COPY crates/cipher-types/Cargo.toml crates/cipher-types/Cargo.toml
COPY src-tauri/Cargo.toml src-tauri/Cargo.toml

RUN cargo fetch --locked

COPY apps/cipher-server apps/cipher-server
COPY crates/cipher-test-support crates/cipher-test-support
COPY crates/cipher-types crates/cipher-types

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
