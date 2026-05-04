# Stage 1: Build
FROM rust:latest as builder

WORKDIR /app
COPY . .

RUN rustup target add x86_64-unknown-linux-musl && \
    cargo build --release --target x86_64-unknown-linux-musl

# Stage 2: Runtime
FROM alpine:latest

RUN apk add --no-cache ca-certificates

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/scaleway-operator /usr/local/bin/

USER nobody

ENTRYPOINT ["scaleway-operator"]
