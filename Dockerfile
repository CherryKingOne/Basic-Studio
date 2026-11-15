# Basic-Studio 主镜像：Rust 控制面 + Python sidecar 运行时
# 构建：docker build -t basic-studio:latest .
# 运行：docker run --rm -p 17283:17283 -v $PWD/models:/models:ro basic-studio:latest

# ---------------------------------------------------------------- builder
FROM rust:1.83-slim AS builder
WORKDIR /build

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY config ./config
COPY sidecars ./sidecars

RUN cargo build --release -p studio-cli \
    && cp target/release/studio /build/studio

# ---------------------------------------------------------------- runtime
FROM ubuntu:22.04 AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 python3 python3-pip ffmpeg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/studio /usr/local/bin/studio
COPY config /app/config
COPY sidecars /app/sidecars

ENV STUDIO_CONFIG=/app/config/default.toml
ENV RUST_LOG=info
EXPOSE 17283

HEALTHCHECK --interval=15s --timeout=3s --start-period=60s \
    CMD studio doctor --config /app/config/default.toml || exit 1

ENTRYPOINT ["studio"]
CMD ["serve", "--start"]