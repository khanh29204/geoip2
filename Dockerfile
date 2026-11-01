FROM rust:1.99.1-alpine AS chef
RUN apk add --no-cache musl-dev \
    && cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
# Keep dependencies in an image layer so remote BuildKit cache exports can reuse them.
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked --bin geoip-rust-service

FROM alpine:3.23
ENV PORT=8080 DB_PATH=/data/GeoLite2-City.mmdb ASN_DB_PATH=/data/GeoLite2-ASN.mmdb
WORKDIR /app
COPY --from=builder /build/target/release/geoip-rust-service /usr/local/bin/geoip-rust-service
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -O /dev/null http://127.0.0.1:8080/health || exit 1
CMD ["geoip-rust-service"]
