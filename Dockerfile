FROM rust:1.78.0 AS builder

WORKDIR /usr/src/app
COPY . .
COPY .git .git

RUN cargo build --release

FROM bitnami/minideb:bookworm

COPY --from=builder /usr/src/app/target/release/lam /bin/lam

CMD ["/bin/lam"]
