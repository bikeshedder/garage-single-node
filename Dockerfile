FROM rust:1.92-alpine AS build

RUN apk add --no-cache \
    build-base \
    pkgconf \
    openssl-dev \
    openssl-libs-static \
    libsodium-dev

WORKDIR /code
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
COPY . .
RUN cargo build --release

FROM dxflrs/garage:v2.1.0 AS garage

FROM scratch
COPY --from=build /code/target/release/garage-bootstrap /garage-bootstrap
COPY --from=garage /garage /garage

CMD ["/garage-bootstrap"]
