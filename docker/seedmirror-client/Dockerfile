# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.89.0
ARG APP_NAME=seedmirror-client

FROM rust:${RUST_VERSION}-slim-trixie AS build
ARG APP_NAME
WORKDIR /app

COPY . .
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
    openssh-client rsync

COPY --from=build /bin/$APP_NAME /bin/
COPY docker/seedmirror-client/docker-entrypoint.sh /bin/

RUN mkdir /config
WORKDIR /config

ENV PUID=1000
ENV PGID=1000
ENTRYPOINT ["docker-entrypoint.sh"]
