## builder
FROM alpine:3.19 AS builder

WORKDIR /code/bancuh-dns

# install system dependencies
RUN apk add build-base \
    cargo \
    clang \
    clang-dev \
    clang-libs \
    cmake \
    linux-headers \
    openssl-dev \
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
FROM alpine:3.19 AS runtime

# install runtime dependencies
RUN apk add openssl bind libgcc libstdc++

# set default logging, can be overridden
ENV RUST_LOG=info

# copy bind config
COPY named.conf /etc/bind/named.conf

# copy binary
COPY --from=builder /code/bancuh-dns/target/release/bancuh-dns /usr/local/bin/bancuh-dns

# set entrypoint
ENTRYPOINT ["/usr/local/bin/bancuh-dns"]
