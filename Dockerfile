FROM rust:1-alpine3.22 AS builder

WORKDIR /build
RUN apk add --no-cache build-base

COPY app/Cargo.toml app/Cargo.lock ./app/
COPY app/src ./app/src
COPY templates ./templates

WORKDIR /build/app
RUN cargo build --locked --release

FROM alpine:3.22

RUN apk add --no-cache ca-certificates \
    && addgroup -S -g 1000 app \
    && adduser -S -D -H -u 1000 -G app app

WORKDIR /app
COPY --from=builder --chown=1000:1000 /build/app/target/release/starry-cloud /usr/local/bin/starry-cloud
COPY --chown=1000:1000 static ./static

USER 1000:1000
EXPOSE 5000
ENTRYPOINT ["/usr/local/bin/starry-cloud"]
CMD ["serve"]
