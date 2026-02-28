## builder
FROM alpine:3.23 AS builder

WORKDIR /code/bancuh-dns

# install system dependencies
RUN apk add build-base \
    cargo \
    clang \
    clang-dev \
    clang-libs \
    linux-headers \
    rust

# setup build dependencies
RUN cargo init .
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release
RUN rm -rf ./src/

# copy code files
COPY /src/ ./src/

# build code
RUN touch ./src/main.rs
RUN cargo build --release


## runtime
FROM alpine:3.23 AS runtime

# install runtime dependencies
RUN apk add openssl bind libgcc libstdc++

# set default logging, can be overridden
ENV RUST_LOG=info

# copy bind config
COPY named.conf /etc/bind/named.conf

# copy binary
COPY --from=builder /code/bancuh-dns/target/release/bancuh-dns /usr/local/bin/bancuh-dns

# healthcheck - runs inside the container against the internal port
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD dig @127.0.0.1 google.com || exit 1

# set entrypoint
ENTRYPOINT ["/usr/local/bin/bancuh-dns"]
