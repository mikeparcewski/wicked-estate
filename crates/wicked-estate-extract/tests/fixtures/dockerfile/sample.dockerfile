# Stage 1 — fetch dependencies
FROM rust:1.85-slim AS deps
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo fetch

# Stage 2 — build release binary
FROM deps AS builder
COPY src ./src
RUN cargo build --release --locked

# Stage 3 — minimal runtime image
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/wicked-estate /usr/local/bin/wicked-estate
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/wicked-estate"]
CMD ["serve", "--port", "8080"]
