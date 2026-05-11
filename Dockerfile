# syntax=docker/dockerfile:1

# ── Stage 1: Chef — outils de build et cross-compilation ─────────────────────
FROM --platform=$BUILDPLATFORM rust:alpine AS chef
WORKDIR /app

RUN apk add --no-cache musl-dev zig

RUN cargo install --locked cargo-chef cargo-zigbuild

RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl


# ── Stage 2: Planner — calcule la recette des dépendances ────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json


# ── Stage 3: Builder — cache des dépendances + build multi-arch ──────────────
FROM chef AS builder

# Copie uniquement recipe.json : couche invalidée seulement si les dépendances changent
COPY --from=planner /app/recipe.json recipe.json

# Build des dépendances pour les deux architectures (couche cachée ~90% du temps)
RUN cargo chef cook \
      --recipe-path recipe.json \
      --release \
      --zigbuild \
      --target x86_64-unknown-linux-musl \
      --target aarch64-unknown-linux-musl

# Build du projet complet
COPY . .
RUN cargo zigbuild --release \
      --target x86_64-unknown-linux-musl \
      --target aarch64-unknown-linux-musl

# Organisation des binaires par plateforme pour la copie contextuelle
RUN mkdir -p /app/linux && \
    cp target/aarch64-unknown-linux-musl/release/scaleway-operator /app/linux/arm64 && \
    cp target/x86_64-unknown-linux-musl/release/scaleway-operator  /app/linux/amd64


# ── Stage 4: Runtime ──────────────────────────────────────────────────────────
FROM alpine:3.21

RUN apk add --no-cache ca-certificates

# TARGETPLATFORM est injecté par docker buildx : "linux/amd64" ou "linux/arm64"
# Défaut linux/amd64 pour `docker build .` sans --platform
ARG TARGETPLATFORM=linux/amd64

COPY --from=builder /app/${TARGETPLATFORM} /usr/local/bin/scaleway-operator
RUN test -f /usr/local/bin/scaleway-operator || \
    (echo "ERROR: binary not found for platform ${TARGETPLATFORM}" && exit 1)

RUN addgroup -S -g 65532 operator && adduser -S -u 65532 -G operator operator
USER 65532

ENTRYPOINT ["scaleway-operator"]
