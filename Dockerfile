# --- Build stage ---
FROM rust:1.91 AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY migrations/ migrations/

RUN cargo build --release

# --- Runtime stage ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    python3 \
    python3-pip \
    ffmpeg \
    && pip3 install --break-system-packages yt-dlp \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/dal /usr/local/bin/dal

# Default config location — mount your dal.toml here
VOLUME /etc/dal
VOLUME /data

EXPOSE 8550

ENTRYPOINT ["dal"]
CMD ["--config", "/etc/dal/dal.toml"]
