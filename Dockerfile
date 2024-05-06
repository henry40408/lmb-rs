FROM rust:1.77.2-alpine AS builder

WORKDIR /usr/src/app
RUN apk add --no-cache build-base=0.5-r3 git=2.43.0-r0
COPY . .
COPY .git .git

RUN cargo build --release

FROM alpine:3.19.1

RUN apk add --no-cache libgcc=13.2.1_git20231014-r0 tini=0.19.0-r2
COPY --from=builder /usr/src/app/target/release/lam /bin/lam

ENTRYPOINT ["tini", "--"]
CMD ["/bin/lam"]
