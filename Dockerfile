# syntax=docker/dockerfile:1

FROM node:24-alpine AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

FROM rust:1-slim-bookworm AS rust
WORKDIR /src
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p pingushka-hub -p pingushka-agent \
    && cp target/release/pingushka-hub /usr/local/bin/pingushka-hub \
    && cp target/release/pingushka-agent /usr/local/bin/pingushka-agent

FROM debian:bookworm-slim AS agent
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10002 --home-dir /var/lib/pingushka --shell /usr/sbin/nologin pingushka
COPY --from=rust /usr/local/bin/pingushka-agent /usr/local/bin/pingushka-agent
ENTRYPOINT ["pingushka-agent"]

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app --shell /usr/sbin/nologin pingushka
WORKDIR /app
COPY --from=rust /usr/local/bin/pingushka-hub /usr/local/bin/pingushka-hub
COPY --from=web /web/dist /app/static
ENV BIND_ADDR=0.0.0.0:8080 \
    STATIC_DIR=/app/static \
    RUST_LOG=info
USER pingushka
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["pingushka-hub"]
