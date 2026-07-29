# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.97.0
ARG NODE_VERSION=24
ARG APP_NAME=seedmirror-client

FROM node:${NODE_VERSION}-slim AS frontend-build
WORKDIR /app

RUN corepack enable && corepack prepare pnpm@latest --activate

COPY crates/seedmirror-client/src/frontend /app/crates/seedmirror-client/src/frontend
WORKDIR /app/crates/seedmirror-client/src/frontend

RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile && pnpm run build

FROM rust:${RUST_VERSION}-slim-trixie AS build
ARG APP_NAME
WORKDIR /app

COPY . .
COPY --from=frontend-build /app/crates/seedmirror-client/src/frontend/dist /app/crates/seedmirror-client/src/frontend/dist

RUN --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
cargo build --locked --release --bin $APP_NAME
cp ./target/release/$APP_NAME /bin
EOF

FROM debian:trixie-slim AS final
ARG APP_NAME

RUN apt-get update && apt-get install -y \
    openssh-client rsync && \
    rm -rf /var/lib/apt/lists/*

COPY --from=build /bin/$APP_NAME /bin/
COPY docker/seedmirror-client/docker-entrypoint.sh /bin/

RUN mkdir /config
WORKDIR /config

ENV PUID=1000
ENV PGID=1000
ENTRYPOINT ["docker-entrypoint.sh"]
