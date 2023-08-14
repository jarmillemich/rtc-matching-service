FROM rust:1.70 as builder

WORKDIR /usr/src/app

# Install some target so we can run on alpine
RUN rustup target add x86_64-unknown-linux-musl

# Build deps first
COPY Cargo.toml Cargo.lock ./
# Fake main.rs so we can build deps
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl && rm src/main.rs

# Now build our actual code
COPY . .

# Main build
RUN touch src/main.rs && cargo build --release --target x86_64-unknown-linux-musl

# Binary image
FROM alpine:latest
WORKDIR /usr/app
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/rtc-matching-service /usr/app/rtc-matching-service

# Copy static files
COPY static /usr/app/static

EXPOSE 3000

CMD ["./rtc-matching-service"]