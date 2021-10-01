FROM rust:1.55.0 as builder

ARG ARCHITECTURE=x86_64-unknown-linux-musl

RUN apt-get update && \
    apt-get install -y musl-tools && \
    rm -rf /var/lib/apt/lists/* && \
    rustup target add "${ARCHITECTURE}"

WORKDIR /builder

COPY Cargo.toml Cargo.lock ./
RUN cargo fetch --locked -v

COPY ./src ./src
COPY ./templates ./templates

RUN cargo build --release --target "${ARCHITECTURE}"

# Runtime image
FROM alpine:3.14.2

ARG ARCHITECTURE=x86_64-unknown-linux-musl

# Add certificates for HTTPS to work (needed for alpine)
RUN apk add --no-cache ca-certificates curl
ENV SSL_CERT_DIR /etc/ssl/certs

WORKDIR /app

COPY --from=builder /builder/target/${ARCHITECTURE}/release/wiki .

ENV RUST_LOG info

CMD ["/app/wiki"]
