# syntax=docker/dockerfile:1.7
FROM node:22-bookworm-slim AS frontend
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1.88-bookworm AS backend
WORKDIR /build/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/backend/target \
    cargo build --locked --release && \
    cp target/release/qzone-archive-web /tmp/qzone-archive-web

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 qzone && useradd --uid 10001 --gid qzone --no-create-home --shell /usr/sbin/nologin qzone && \
    mkdir -p /app/public /data/jobs && chown -R qzone:qzone /data/jobs && chmod 0700 /data/jobs
COPY --from=backend /tmp/qzone-archive-web /usr/local/bin/qzone-archive-web
COPY --from=frontend /build/frontend/dist /app/public
ENV QZONE_BIND=0.0.0.0:8091 \
    QZONE_FRONTEND_DIR=/app/public \
    QZONE_DATA_DIR=/data/jobs \
    RUST_LOG=qzone_archive_web=info,tower_http=info
USER qzone:qzone
EXPOSE 8091
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:8091/api/health >/dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/qzone-archive-web"]
