## builder
FROM rust:1.76-bookworm AS builder

WORKDIR /code/bancuh-dns

# install system dependencies
RUN apt-get update
RUN apt-get install -y clang

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
FROM debian:bookworm AS runtime

# install runtime dependencies
RUN apt-get update
RUN apt-get install -y openssl libc6 libstdc++6 bind9

# set default logging, can be overridden
ENV RUST_LOG=info

# copy bind config
COPY named.conf.options /etc/bind/named.conf.options

# copy binary
COPY --from=builder /code/bancuh-dns/target/release/bancuh-dns /usr/local/bin/bancuh-dns

# set entrypoint
ENTRYPOINT ["/usr/local/bin/bancuh-dns"]
