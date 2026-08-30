# Bancuh DNS

Bancuh Adblock DNS server written in Rust

## Introduction

A DNS server resolves domain names into IP addresses. An adblock DNS server does the same, but intercepts queries for known ad/tracker domains and returns a null IP (`0.0.0.0` / `::`) instead of forwarding them — effectively blocking them at the network level for any device using the server.

`bancuh-dns` is a self-contained adblock DNS server written in Rust.

Key strengths:

- Easy deployment using Docker
- Low memory footprint — around 100 MB for the server plus 25–50 MB for the bundled recursor
- Automatic daily blocklist updates (configurable via `UPDATE_INTERVAL`)
- Custom blacklist, whitelist, and domain rewrite support
- Privacy-first: defaults to a local [unbound](https://nlnetlabs.nl/projects/unbound/) recursive resolver with DNSSEC validation (no queries sent to public DNS)
- Optional custom forwarders (e.g. `1.1.1.1`) via `FORWARDERS` env var

Additionally, and **experimentally**, the server can terminate DoT/DoH itself, rate-limit
per client IP, and serve a query-log UI. See [Deployment modes](#deployment-modes) for when
those apply — they are off by default and not used in production.

## Deployment modes

`bancuh-dns` can run in two arrangements. Which one you pick determines which features are
available and which are meaningful, so it is worth deciding up front.

### Backend mode (recommended)

`bancuh-dns` handles plain DNS on port 53 and nothing else. A separate front-end —
[dnsdist-acme](https://github.com/ragibkl/dnsdist-acme) — terminates DoT and DoH, manages
Let's Encrypt certificates, applies rate limiting, and serves query logs.

```
client ──▶ dnsdist-acme ──▶ bancuh-dns ──▶ unbound / FORWARDERS
           (:53 :443 :853)   (:1153)
           TLS, ACME,        filtering
           rate limit, logs
```

This is what the public Bancuh DNS service runs on every node, and it is the arrangement
the project is tested against. Use it unless you have a specific reason not to.

In this mode, leave `TLS_ENABLED`, `ADMIN_ENABLED` and `RATE_LIMIT_ENABLED` unset — the
front-end provides all three, and enabling them here is at best redundant and at worst
harmful (see the warnings on each below).

### Standalone mode (experimental)

`bancuh-dns` binds the public interface directly and terminates DoT/DoH itself, obtaining
its own certificate over ACME.

This mode is **not production-tested**. Two known problems:

- **UDP does not work behind a NAT-style floating/reserved IP** (e.g. a DigitalOcean
  Reserved IP). Replies are sent from the host's primary address rather than the address
  the query arrived on, and clients discard them. TCP, DoT and DoH are unaffected.
  See [#8](https://github.com/ragibkl/bancuh-dns/issues/8).
- **DoH supports POST only.** RFC 8484 also defines a GET form (`?dns=<base64url>`), which
  browsers and some monitoring tools use. Those clients will fail.

Use it for local experimentation, or where the server has a normal, directly-bound public
address and you only need DoT / POST-based DoH.

## Architecture

```
                        ┌──────────────────────────────────────────────────┐
                        │                  bancuh-dns                      │
                        │                                                  │
 DNS/UDP (port 53)  ───▶│  Handler                                         │
 DNS/TCP (port 53)  ───▶│    0. rate limit?  ──▶ dropped   [experimental]  │
 DoT  (853) [exp.]  ───▶│    1. rewrite?     ──▶ forward alias             │
 DoH  (443) [exp.]  ───▶│    2. blocked?     ──▶ return 0.0.0.0 / ::       │
                        │    3. passthrough  ──▶ Resolver                  │
                        │    ──▶ log query   [experimental]                │
                        │                          │                       │
                        │  AdblockEngine           │                       │
                        │  (ArcSwap<DB>)           ▼                       │
                        │    blacklist      FORWARDERS set?                │
                        │    whitelist       yes ──▶ upstream IPs          │
                        │    rewrites        no  ──▶ local unbound         │
                        │                                                  │
 HTTP (8080) [exp.] ───▶│  Admin server (query logs UI + JSON API)         │
                        │                                                  │
                        │  Update loop (daily)                             │
                        │    fetch config ──▶ compile ──▶ swap DB          │
                        │                                                  │
                        │  ACME loop  [experimental, TLS_ENABLED only]     │
                        │    HTTP-01 challenge server (port 80)            │
                        │    obtain/renew cert from Let's Encrypt          │
                        └──────────────────────────────────────────────────┘
```

Components marked `[experimental]` belong to [standalone mode](#standalone-mode-experimental)
and are disabled by default.

### Components

| Component | Role |
|---|---|
| `hickory-server` | DNS server — UDP + TCP port 53; DoT 853 and DoH 443 in standalone mode |
| `AdblockEngine` | Holds the active blocklist DB; swapped atomically on update |
| `AdblockDB` | Three RocksDB stores: `blacklist`, `whitelist`, `rewrites` |
| `Resolver` | Forwards allowed queries to upstream DNS |
| `unbound` | Local recursive resolver used when no `FORWARDERS` are set. Listens on `127.0.0.1:5353` only, validates DNSSEC against a root anchor primed at image build time |
| Rate limiter | _Experimental._ Per-IP token bucket (`governor`) — silently drops excess queries. Off unless `RATE_LIMIT_ENABLED` |
| Query log | _Experimental._ In-memory per-IP log store with 10-minute retention. Off unless `ADMIN_ENABLED` |
| Admin server | _Experimental._ HTTP UI + JSON API for viewing your own query logs. Off unless `ADMIN_ENABLED` |
| Update loop | Fetches config, compiles a fresh DB, hot-swaps it with zero downtime |
| ACME loop | _Experimental._ Obtains and renews TLS certs via Let's Encrypt HTTP-01 challenge |

### Request flow

1. Query arrives → **rate limit check** (only if `RATE_LIMIT_ENABLED`) → silently dropped
2. `Handler` looks up the domain in `AdblockEngine`
3. **Rewrite match** → returns a CNAME to the alias, then resolves the alias
4. **Blacklist match** (and not whitelisted) → returns `0.0.0.0` (A) or `::` (AAAA)
5. **No match** → forwarded to upstream resolver (unbound or `FORWARDERS`)
6. Query is recorded in the in-memory log store, if `ADMIN_ENABLED`

### Response codes

Upstream outcomes are passed through rather than flattened, so clients can tell a failure
from an empty answer and fail over to a secondary resolver when appropriate:

| Upstream result | Returned to client |
|---|---|
| Records found | `NOERROR` with answers |
| Name exists, no records of this type (NODATA) | `NOERROR` with no answers |
| Name does not exist | `NXDOMAIN` |
| SERVFAIL, REFUSED, FORMERR, timeout, … | `SERVFAIL` |

Note that a real failure is never reported as `NXDOMAIN`: clients cache negative answers,
so doing that would make a transient upstream problem sticky.

### Blocklist updates

On startup and then every `UPDATE_INTERVAL` seconds (default: 86400), the update loop:
1. Fetches `configuration.yaml` from `CONFIG_URL`
2. Downloads all configured blacklist/whitelist/rewrite sources
3. Compiles them into a fresh RocksDB instance
4. Atomically swaps the new DB into the engine — in-flight queries are unaffected
5. On failure: logs a warning, keeps the existing DB, retries next interval

The server answers queries throughout, but see [Known limitations](#known-limitations) for
the query latency caused by the compile step.

## Configuration

### Core

| Env var | Default | Description |
|---|---|---|
| `CONFIG_URL` | upstream GitHub config | URL or file path to `configuration.yaml` |
| `PORT` | `53` | DNS listener port |
| `FORWARDERS` | _(unset)_ | Comma-separated upstream DNS IPs. If unset, uses the local unbound recursor |
| `FORWARDERS_PORT` | `53` | Port for upstream forwarders |
| `UPDATE_INTERVAL` | `86400` | Blocklist refresh interval in seconds |

### Admin / query logs (experimental)

> **Standalone mode only.** The log store is keyed by the **DNS query source address**,
> while `/logs` matches against the address of the **browser** requesting the page. In
> backend mode the former is always the front-end proxy, so the two never match and every
> visitor sees an empty table.

Set `ADMIN_ENABLED=true` to enable the query-log HTTP server. It serves `/logs` (HTML)
and `/api/logs` (JSON), each showing only the queries made by the caller's own IP
address. When disabled, no log store is kept and no port is bound.

| Env var | Default | Description |
|---|---|---|
| `ADMIN_ENABLED` | `false` | Enable the admin HTTP server and query-log store |
| `ADMIN_PORT` | `8080` | Port for the admin HTTP server (also 8443 for HTTPS when `TLS_ENABLED`) |

### Rate limiting (experimental)

> **Standalone mode only.** The limiter keys on the DNS query source address. Behind a
> front-end proxy every query arrives from the same address, so the whole server shares a
> single bucket — setting `RATE_LIMIT=100` would cap the entire server at 100 queries per
> second while appearing to work normally. Leave this off in backend mode and rate-limit
> at the front-end instead.

Set `RATE_LIMIT_ENABLED=true` to enable per-IP rate limiting. Excess queries are dropped
silently rather than refused, to avoid reflecting traffic at spoofed source addresses.

| Env var | Default | Description |
|---|---|---|
| `RATE_LIMIT_ENABLED` | `false` | Enable per-IP rate limiting |
| `RATE_LIMIT` | `100` | Max DNS requests per second per IP prefix |
| `RATE_LIMIT_IPV4_PREFIX` | `32` | IPv4 prefix length for rate limiting (32 = per-IP, 24 = per /24 subnet) |
| `RATE_LIMIT_IPV6_PREFIX` | `48` | IPv6 prefix length for rate limiting (48 = per /48 block, 128 = per-IP) |

### TLS / ACME (experimental)

> **Standalone mode only**, and subject to the two limitations described there — UDP is
> broken behind a floating/reserved IP, and DoH accepts POST but not GET.
>
> If a front-end already terminates TLS, make sure `TLS_ENABLED` is not inherited from a
> shared env file: `bancuh-dns` reads the same variable name, and will start its own ACME
> run and fail HTTP-01 in a retry loop, which counts against Let's Encrypt's failed
> validation limit for the domain.

Set `TLS_ENABLED=true` to enable DoT (port 853) and DoH (port 443). The server will
automatically obtain and renew a certificate from Let's Encrypt using the HTTP-01
challenge (served on port 80).

| Env var | Default | Description |
|---|---|---|
| `TLS_ENABLED` | `false` | Enable DoT + DoH with automatic ACME cert management |
| `TLS_DOMAIN` | _(required)_ | Domain name for the TLS certificate |
| `TLS_EMAIL` | _(required)_ | Email for Let's Encrypt account registration |
| `ACME_URL` | LE production | Custom ACME directory URL (e.g. LE staging for testing) |
| `ACME_CACHE_DIR` | `/var/cache/bancuh-dns/certs` | Directory to cache the cert and account key across restarts |
| `ACME_INSECURE` | `false` | Disable TLS verification for the ACME server (for local Pebble testing only) |

## Known limitations

- **Query latency during blocklist compile.** The compile writes every domain to RocksDB
  on the async runtime without yielding. On a single-core host this starves the DNS
  handler, causing intermittent timeouts for the duration of the compile (roughly two to
  three minutes, once per `UPDATE_INTERVAL`). See
  [#9](https://github.com/ragibkl/bancuh-dns/issues/9).
- **No blocking until the first compile finishes.** After a restart the server answers
  immediately but with an empty database, so nothing is blocked until the first update
  completes.
- **Standalone mode:** UDP behind a floating/reserved IP, and DoH GET — see
  [Deployment modes](#standalone-mode-experimental).

## Image tags

Use **`ragibkl/bancuh-dns:2`**.

The `latest` tag is intentionally frozen at an older build and no longer tracks `master`.
It predates the TLS, rate-limiting and query-log work, and is kept unchanged so that
existing deployments pulling the untagged image keep the behaviour they were installed
with. New deployments should pin `:2`.

## Getting started

The best way to run this project is via Docker Compose. See the
[Adblock DNS Server](https://github.com/ragibkl/adblock-dns-server) repo for a
ready-made configuration and getting-started guide.

### Quick local run (plain DNS)

```yaml
# docker-compose.yaml
services:
  dns:
    image: ragibkl/bancuh-dns:2
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

### With DoT + DoH (TLS, experimental)

> Standalone mode — read [its limitations](#standalone-mode-experimental) first. In
> particular, do not use this on a host whose public address is a floating/reserved IP:
> UDP DNS will not work.

Requires a public domain pointing to your server (for Let's Encrypt HTTP-01 validation).
Ports 80, 443, and 853 must be reachable from the internet.

```yaml
services:
  dns:
    image: ragibkl/bancuh-dns:2
    restart: always
    cap_add:
      - NET_BIND_SERVICE
    network_mode: host
    environment:
      CONFIG_URL: /data/configuration.yaml
      TLS_ENABLED: "true"
      TLS_DOMAIN: "dns.example.com"
      TLS_EMAIL: "admin@example.com"
    volumes:
      - ./data:/data
      - certs:/var/cache/bancuh-dns/certs

volumes:
  certs:
```

## Related repos

- [adblock-dns-server](https://github.com/ragibkl/adblock-dns-server) — blocklist data and deployment config
- [dnsdist-acme](https://github.com/ragibkl/dnsdist-acme) — TLS/DoH/DoT front-end used in backend mode
