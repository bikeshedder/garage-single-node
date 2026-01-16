ARG GARAGE_VERSION
ARG RUST_VERSION

FROM rust:${RUST_VERSION:?}-alpine AS build

RUN apk add --no-cache \
    build-base \
    pkgconf \
    openssl-dev \
    openssl-libs-static \
    libsodium-dev

WORKDIR /code
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -r src
COPY . .
RUN cargo build --release

FROM dxflrs/garage:v${GARAGE_VERSION:?} AS garage

FROM scratch
COPY --from=build /code/target/release/garage-bootstrap /garage-bootstrap
COPY --from=garage /garage /garage

CMD ["/garage-bootstrap"]
