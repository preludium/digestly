# syntax=docker/dockerfile:1

# ---- Stage 1: build the frontend to static assets ----
FROM node:20-bookworm-slim AS web
WORKDIR /web
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm run build   # -> /web/dist

# ---- Stage 2: build the Rust backend ----
FROM rust:1.88-slim-bookworm AS server
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config build-essential ca-certificates libssl-dev \
    && rm -rf /var/lib/apt/lists/*
# Cache dependency compilation separately from source.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true \
    && rm -rf src
# Real source + migrations (sqlx::migrate! embeds ./migrations at compile time).
COPY migrations ./migrations
COPY src ./src
RUN touch src/main.rs && cargo build --release
RUN strip target/release/digestly || true

# ---- Stage 3: slim runtime ----
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /app/target/release/digestly /usr/local/bin/digestly
COPY --from=web /web/dist /app/static
ENV DATA_DIR=/data \
    BIND_ADDR=0.0.0.0:8080 \
    STATIC_DIR=/app/static \
    RUST_LOG=info,digestly=debug
EXPOSE 8080
VOLUME ["/data"]
# SIGTERM is delivered directly to the binary (PID 1) for graceful shutdown.
ENTRYPOINT ["/usr/local/bin/digestly"]
