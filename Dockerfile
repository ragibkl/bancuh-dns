## builder
FROM rust AS builder

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
FROM debian:12 AS runtime

RUN apt-get update
RUN apt-get install -y openssl libc6 libstdc++6

# set default logging, can be overridden
ENV RUST_LOG=info

# copy binary
COPY --from=builder /code/bancuh-dns/target/release/bancuh-dns /usr/local/bin/bancuh-dns

# set entrypoint
ENTRYPOINT ["/usr/local/bin/bancuh-dns"]
