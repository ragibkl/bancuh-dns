# Bancuh DNS

Bancuh Adblock DNS server written in Rust

## Introduction

A DNS server resolves domain names into IP addresses. An adblock DNS server does the same, but intercepts queries for known ad/tracker domains and returns a null IP (`0.0.0.0` / `::`) instead of forwarding them — effectively blocking them at the network level for any device using the server.

`bancuh-dns` is a self-contained adblock DNS server written in Rust.

Key strengths:

- Easy deployment using Docker
- Low memory footprint (< 300 MB RAM)
- Automatic daily blocklist updates (configurable via `UPDATE_INTERVAL`)
- Custom blacklist, whitelist, and domain rewrite support
- Privacy-first: defaults to a local BIND9 recursive resolver (no queries sent to public DNS)
- Optional custom forwarders (e.g. `1.1.1.1`) via `FORWARDERS` env var

## Architecture

```
                        ┌─────────────────────────────────────────┐
                        │               bancuh-dns                │
                        │                                         │
 DNS query (port 53) ──▶│  Handler                                │
                        │    1. rewrite?  ──▶ forward alias       │
                        │    2. blocked?  ──▶ return 0.0.0.0      │
                        │    3. passthrough ──▶ Resolver           │
                        │                          │              │
                        │  AdblockEngine           │              │
                        │  (ArcSwap<DB>)           ▼              │
                        │    blacklist      FORWARDERS set?        │
                        │    whitelist       yes ──▶ upstream IPs  │
                        │    rewrites        no  ──▶ local BIND9   │
                        │                                         │
                        │  Update loop (daily)                    │
                        │    fetch config ──▶ compile ──▶ swap DB  │
                        └─────────────────────────────────────────┘
```

### Components

| Component | Role |
|---|---|
| `hickory-server` | DNS server, listens on TCP + UDP port 53 |
| `AdblockEngine` | Holds the active blocklist DB; swapped atomically on update |
| `AdblockDB` | Three RocksDB stores: `blacklist`, `whitelist`, `rewrites` |
| `Resolver` | Forwards allowed queries to upstream DNS |
| `bind` (BIND9) | Local recursive resolver used when no `FORWARDERS` are set |
| Update loop | Fetches config, compiles a fresh DB, hot-swaps it with zero downtime |

### Request flow

1. Query arrives → `Handler` looks up the domain in `AdblockEngine`
2. **Rewrite match** → returns a CNAME to the alias, then resolves the alias
3. **Blacklist match** (and not whitelisted) → returns `0.0.0.0` (A) or `::` (AAAA)
4. **No match** → forwarded to upstream resolver (BIND9 or `FORWARDERS`)

### Blocklist updates

On startup and then every `UPDATE_INTERVAL` seconds (default: 86400), the update loop:
1. Fetches `configuration.yaml` from `CONFIG_URL`
2. Downloads all configured blacklist/whitelist/rewrite sources
3. Compiles them into a fresh RocksDB instance
4. Atomically swaps the new DB into the engine — in-flight queries are unaffected
5. On failure: logs a warning, keeps the existing DB, retries next interval

## Configuration

| Env var | Default | Description |
|---|---|---|
| `CONFIG_URL` | upstream GitHub config | URL or file path to `configuration.yaml` |
| `PORT` | `53` | DNS listener port |
| `FORWARDERS` | _(unset)_ | Comma-separated upstream DNS IPs. If unset, uses local BIND9 |
| `FORWARDERS_PORT` | `53` | Port for upstream forwarders |
| `UPDATE_INTERVAL` | `86400` | Blocklist refresh interval in seconds |

## Getting started

The best way to run this project is via Docker Compose. See the
[Adblock DNS Server](https://github.com/ragibkl/adblock-dns-server) repo for a
ready-made configuration and getting-started guide.

### Quick local run

```yaml
# docker-compose.yaml
services:
  dns:
    image: ragibkl/bancuh-dns
    restart: always
    environment:
      CONFIG_URL: /data/configuration.yaml
      PORT: 53
      # FORWARDERS: "1.1.1.1,1.0.0.1"
    volumes:
      - ./data:/data
    ports:
      - 1153:53/tcp
      - 1153:53/udp
```

## Related repos

- [adblock-dns-server](https://github.com/ragibkl/adblock-dns-server) — blocklist data and deployment config
- [dnsdist-acme](https://github.com/ragibkl/dnsdist-acme) — optional DoH/DoT/ACME frontend proxy
