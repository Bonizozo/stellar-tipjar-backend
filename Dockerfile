# syntax=docker/dockerfile:1
# --- Builder Stage ---
FROM rust:latest AS builder

WORKDIR /app

# .cargo/config.toml pins this workspace to build for x86_64-pc-windows-gnu by
# default (for Windows/Git-Bash contributors). Override it here so the image
# builds a native Linux binary that can actually run in the runtime stage.
ENV CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu
# Use the committed .sqlx/ query metadata instead of connecting to a live
# database during the build.
ENV SQLX_OFFLINE=true

COPY . .

# Cache the cargo registry/git index and the incremental build output across
# builds. Both mounts are keyed by this Dockerfile's build context, so a
# rebuild after only touching a handful of source files reuses almost all of
# the previously-compiled dependency graph instead of recompiling from
# scratch.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --target x86_64-unknown-linux-gnu && \
    cp target/x86_64-unknown-linux-gnu/release/stellar-tipjar-backend /app/stellar-tipjar-backend

# --- Runtime Stage ---
FROM debian:bookworm-slim

# Install necessary runtime libraries
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/stellar-tipjar-backend .
COPY --from=builder /app/migrations ./migrations

# Expose port
EXPOSE 8000

# Run the app
CMD ["./stellar-tipjar-backend"]
