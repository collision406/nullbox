# syntax=docker/dockerfile:1.7

# Nullbox in one image graph: a static SPA served by Caddy, and the API beside
# it. Caddy is the only published service, so the browser talks to one origin
# and the API is never exposed directly.

# --- web bundle -------------------------------------------------------------
FROM node:22-bookworm-slim AS web-build
WORKDIR /src
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --frozen-lockfile
COPY apps/web ./apps/web
# Empty base URL = call /api on the origin that served the page. A value would
# only be right for a split deployment (CDN + separate API), which this is not.
ARG VITE_API_BASE_URL=""
ENV VITE_API_BASE_URL=${VITE_API_BASE_URL}
RUN pnpm --filter web build

# --- api binary -------------------------------------------------------------
FROM rust:1.99.1-bookworm AS api-build
WORKDIR /src
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential \
 && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
COPY .sqlx ./.sqlx
# Build from the committed sqlx cache; no database is needed at compile time.
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p nullbox-api \
 && cp target/release/nullbox-api /tmp/nullbox-api

# --- api runtime ------------------------------------------------------------
FROM debian:bookworm-slim AS api
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 nullbox \
 && useradd --system --uid 10001 --gid nullbox --home-dir /app --shell /usr/sbin/nologin nullbox \
 && install -d -o nullbox -g nullbox /app/data
WORKDIR /app
COPY --from=api-build /tmp/nullbox-api /usr/local/bin/nullbox-api
ENV BIND_ADDR=0.0.0.0:3000 \
    DATABASE_URL=sqlite://data/nullbox.db \
    RUST_LOG=info,tower_http=info
USER nullbox
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:3000/api/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/nullbox-api"]

# --- web + TLS reverse proxy ------------------------------------------------
FROM caddy:2-alpine AS web
COPY deploy/Caddyfile /etc/caddy/Caddyfile
COPY --from=web-build /src/apps/web/dist /srv/web
