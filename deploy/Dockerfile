# syntax=docker/dockerfile:1
# SPDX-License-Identifier: AGPL-3.0-only
# Build from the repository root:  docker build -f deploy/Dockerfile -t opnrelay .

# ---- Web UI ------------------------------------------------------------------
FROM node:22-bookworm-slim AS web
WORKDIR /src/apps/web
RUN corepack enable
COPY apps/web/package.json apps/web/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store pnpm install --frozen-lockfile
COPY apps/web/ ./
RUN pnpm build

# ---- Server ------------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
# Workspace member manifests (not built into the server image).
COPY apps/desktop apps/desktop
COPY --from=web /src/apps/web/dist apps/web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p opnrelay-server \
 && cp target/release/opnrelay-server /opnrelay-server \
 && mkdir -p /data/media

# ---- Runtime: distroless, non-root, no shell -----------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.title="OpnRelay" \
      org.opencontainers.image.description="Self-hosted step-by-step guide capture" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      org.opencontainers.image.source="https://github.com/stevy2191/opnrelay"
COPY --from=build /opnrelay-server /usr/local/bin/opnrelay-server
COPY --from=build --chown=65532:65532 /data /data
# Desktop recorder installers and extension packages (filled by the release
# workflow; empty in local builds). Offered to admins on the Downloads page.
COPY deploy/downloads/ /opt/opnrelay/downloads/
ENV OPNRELAY_BIND=0.0.0.0:8080 \
    OPNRELAY_STORAGE=fs:///data/media \
    OPNRELAY_BACKUP_DIR=/data/backups \
    OPNRELAY_DOWNLOADS_DIR=/opt/opnrelay/downloads \
    OPNRELAY_LOG_FORMAT=json
USER 65532:65532
EXPOSE 8080
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s CMD ["/usr/local/bin/opnrelay-server", "health"]
ENTRYPOINT ["/usr/local/bin/opnrelay-server"]
CMD ["serve"]
