# Build image for the AppImage's frame binary: an old glibc (Ubuntu 22.04,
# 2.35) so the binary runs on older distros too. build.sh passes the pins
# from versions.env; the container runs as the calling user.
ARG BUILD_IMAGE
FROM ${BUILD_IMAGE}

ARG RUSTUP_INIT_URL
ARG RUSTUP_INIT_SHA256
ARG RUST_TOOLCHAIN

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates curl gcc libc6-dev \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rust/rustup PATH=/opt/rust/cargo/bin:$PATH
RUN curl -fsSL -o /tmp/rustup-init "$RUSTUP_INIT_URL" \
 && echo "$RUSTUP_INIT_SHA256  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && CARGO_HOME=/opt/rust/cargo /tmp/rustup-init -y --no-modify-path \
      --profile minimal --default-toolchain "$RUST_TOOLCHAIN" \
 && rm /tmp/rustup-init \
 && chmod -R a+rX /opt/rust
