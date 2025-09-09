# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.89.0
ARG APP_NAME=seedmirror-client

FROM rust:${RUST_VERSION}-slim-trixie AS build
ARG APP_NAME
WORKDIR /app

RUN --mount=type=bind,source=.,target=. \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
cargo build --locked --release --bin $APP_NAME
cp ./target/release/$APP_NAME /bin/app
EOF

FROM debian:trixie-slim AS final

RUN apt-get update && apt-get install -y \
    openssh-client rsync

ARG UID=10001
RUN useradd \
    --uid "${UID}" \
    --home-dir /nonexistent \
    --shell /sbin/nologin \
    --no-create-home \
    --comment "" \
    appuser
USER appuser

COPY --from=build /bin/app /bin/

ENV RUST_LOG=seedmirror_client=info
ENTRYPOINT ["/bin/app"]
