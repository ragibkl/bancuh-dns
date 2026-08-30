## builder
FROM alpine:3.24 AS builder

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
FROM alpine:3.24 AS runtime

# install runtime dependencies
# bind-tools provides dig, used by the HEALTHCHECK below. It used to arrive
# transitively via the bind package, so it must now be requested explicitly.
RUN apk add unbound bind-tools libgcc libstdc++

# set default logging, can be overridden
ENV RUST_LOG=info

# copy unbound config
COPY unbound.conf /etc/unbound/unbound.conf

# prime the DNSSEC root trust anchor. unbound does not validate without one,
# and unbound-anchor exits 1 when it writes a new key, which is not an error.
RUN unbound-anchor -a /etc/unbound/root.key || true; \
    unbound-checkconf /etc/unbound/unbound.conf

# copy binary
COPY --from=builder /code/bancuh-dns/target/release/bancuh-dns /usr/local/bin/bancuh-dns

# healthcheck - runs inside the container against the internal port
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD dig @127.0.0.1 google.com || exit 1

# set entrypoint
ENTRYPOINT ["/usr/local/bin/bancuh-dns"]
