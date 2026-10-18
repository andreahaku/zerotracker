# Cross build environment for the CardputerZero (aarch64, Debian 13 trixie).
#
# Adapted from ZeroRadio's docker/cp0-build. Building against trixie's own arm64
# libraries with trixie's cross gcc gives binaries whose glibc/libstdc++
# requirements match the device. Rust comes from rustup, pinned to the host
# toolchain version, with the aarch64-unknown-linux-gnu target.
FROM debian:trixie

ENV DEBIAN_FRONTEND=noninteractive

RUN dpkg --add-architecture arm64 \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
        crossbuild-essential-arm64 gcc libc6-dev \
        cmake ninja-build git ca-certificates curl pkg-config file dpkg-dev python3 \
        libfreetype-dev:arm64 \
        libpng-dev:arm64 \
        libjpeg-dev:arm64 \
        zlib1g-dev:arm64 \
        libdrm-dev:arm64 \
        libasound2-dev:arm64 \
        libfmt-dev:arm64 \
 && rm -rf /var/lib/apt/lists/*

ARG RUST_VERSION=1.98.1
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
        --default-toolchain "$RUST_VERSION" --target aarch64-unknown-linux-gnu \
 && chmod -R a+rwX /opt/rustup /opt/cargo

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
