# syntax=docker/dockerfile:1

FROM rust:1.98.1-slim-bookworm AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY backend/ backend/
WORKDIR /app/backend
RUN cargo build --release --locked --no-default-features --features postgres

FROM debian:bookworm-20260918-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 talos \
    && useradd --system --uid 10001 --gid talos --home-dir /app --shell /usr/sbin/nologin talos
WORKDIR /app
COPY --from=builder /app/backend/target/release/talos-backend /app/talos-backend
RUN mkdir -p /app/public && chown -R talos:talos /app
EXPOSE 8080
ENV PUBLIC_DIR=/app/public
USER talos
CMD ["/app/talos-backend"]
