# How bancuh-dns compares

Notes from running the same blocklist through four DNS filtering engines, to
work out which of them can actually hold a list this size.

This is not a scoreboard. Pi-hole, Blocky and AdGuard Home are all good at what
they are built for, and for most people one of them is the better choice —
they have web UIs, per-client policy, installers and communities that this
project does not. The interesting question is narrower: **what happens at 7.8
million entries, on a 1 GB VPS.**

That is the constraint this project is built for: 1 GB RAM, ~1 GB swap, 1 vCPU.
Swap does not rescue you here — DNS is latency-critical, and an engine that
pages its blocklist in and out on the query path is worse than one that does
not fit at all.

## The list

The Bancuh blocklist is 7,806,090 entries compiled from 71 sources:

- **4.63M plain domains** — `example.com`
- **3.18M wildcard patterns** — `*.example.com`, blocking a domain and every
  subdomain

The wildcard share matters, and is the single biggest differentiator below.
It comes from the wildcard variants of upstream lists (hagezi and others).

## Results

Same list, same machine, each engine given `1.1.1.1` as upstream so that only
the filtering is being compared and not the resolver.

| | resident memory | on disk | qps as shipped | qps, logging off | wildcard entries | entries loaded |
|---|---|---|---|---|---|---|
| **Pi-hole 6.4.3** | **10 MB** | 259 MB | 6,514 | **54,680** | no | 4.63M |
| **bancuh-dns** | 41 MB | 165 MB | **40,323** | 40,323 | yes | 7.81M |
| **AdGuard Home** | 1198 MB | — | 5,619 | 6,660 | no † | 7.81M |
| **Blocky 0.34** | 608 MB | — | 3,552 | 5,054 | yes | 7.81M |
| Pi-hole, 3.18M wildcards as regex § | ~11 GB § | 259 MB | — | — | yes | 7.81M |

`dnsperf`, 20s, 500 queries outstanding, each engine pinned to **1 vCPU** to
match the target hardware, and all four on Docker host networking so none pays a
NAT penalty the others avoid. AdGuard Home had to be given 2 GB to run at all;
every other engine was capped at 1 GB. Queries were for a blocked domain, so
this measures the filtering path only — no upstream resolution is involved.

"qps as shipped" is each engine at its own defaults. "logging off" disables
per-query logging and telemetry everywhere. bancuh-dns is identical in both
columns because it already ships with per-query logging off; the others ship
with it on.

† In hosts format, which is what it was given. AdGuard Home's native adblock
syntax (`||example.com^`) does express wildcards, so this is a limitation of
the list format used here, not of AdGuard Home.

## What the numbers say

**There are two architectures here, not four.** Pi-hole and bancuh-dns keep the
list on disk and let the OS page cache serve it — 12 MB and 41 MB resident.
Blocky and AdGuard Home hold it in memory — 624 MB and 1186 MB. At this list
size that is the difference between fitting on a 1 GB host alongside a recursor
and a TLS front-end, and not fitting.

**On the target hardware, the full stack has to fit.** bancuh-dns does not run
alone — a recursor and a TLS front-end sit alongside it. Measured on a
production node:

| stack | resident total | fits in 1 GB? |
|---|---|---|
| bancuh-dns 41 + unbound 149 + dnsdist 47 | **~240 MB** | yes, comfortably |
| Blocky 624 + unbound 149 + dnsdist 47 | ~820 MB | technically, with nothing spare |
| AdGuard Home 1186 (+ front-end) | >1.2 GB | no — into swap |

The Blocky row is the one worth dwelling on. It fits on paper, but leaves no
room for page cache, which is what makes on-disk lookups fast in the first
place. AdGuard Home exceeds the box outright.

**Query latency is a wash.** 127–166 µs across the host-networked three. Real
users see ~39 ms of network round trip, so the filtering step is well under 1%
of what anyone experiences. It is not a useful axis for choosing between these.

**Logging defaults dominate throughput, everywhere.** This was the largest
single effect measured, and it is not architectural — it is configuration:

| engine | as shipped | logging off | cost of default logging |
|---|---|---|---|
| Pi-hole | 6,514 | 54,680 | **88%** |
| Blocky | 3,552 | 5,054 | 30% |
| AdGuard Home | 5,619 | 6,660 | 16% |
| bancuh-dns (forced to `RUST_LOG=info`) | 23,726 | 40,323 | 41% |

Pi-hole is the extreme case: writing a row per query to its long-term SQLite
history costs it seven eighths of its throughput. That is a reasonable trade for
a household appliance whose dashboards are the point, and a poor one for a
public resolver. bancuh-dns ships with per-query logging off for that reason, and
because it does not retain query history at all.

Anyone comparing DNS filters should equalise this before drawing conclusions.
Measured at defaults, the numbers largely reflect each project's telemetry
choices rather than the engine underneath.

**The split is by implementation language, not by storage.** With logging
equalised, the C and Rust engines (Pi-hole, bancuh-dns) reach 40–55k qps on one
core, while the Go engines (Blocky, AdGuard Home) reach 5–7k — a 6–8x gap.

Storage location turned out to predict *memory* but not *throughput*. The two
on-disk engines use 10 MB and 41 MB against 608 MB and 1198 MB for the
in-memory pair, and they are also the two fastest — but the ordering within each
pair does not follow from where the data lives.

Garbage collection is an obvious suspect for the Go gap, since both hold
600 MB–1.2 GB live heaps and tracing that competes with query serving on a
single core. That remains unverified here — measuring it properly would mean
profiling the Go runtimes rather than inferring from the outcome.

**Pi-hole is the fastest engine measured, and it is doing less work.** It loaded
4.63M entries against 7.81M because it rejected every wildcard, and its blocked
path is a single exact-match lookup where bancuh-dns probes up to one key per
label across three stores. Part of the gap between 54,680 and 40,323 is the cost
of wildcard support rather than inefficiency.

**Wildcards split the field.** Only bancuh-dns and Blocky matched
`x.y.doubleclick.net` from a `*.doubleclick.net` entry. Pi-hole rejects `*`
entries outright at import, which is the entire 7.81M → 4.71M gap: it silently
loaded 60% of the list.

**§ Pi-hole's regex table is not a substitute** for bulk wildcards. Gravity is
an indexed lookup; the regex table is compiled into memory and evaluated
linearly on every cache-missing query. Both costs scale with the number of
patterns:

| regexes | latency per uncached query | FTL resident memory | |
|---|---|---|---|
| 0 | 51 ms (upstream only) | 2 MB | measured |
| 100,000 | 148 ms | 359 MB | measured |
| 300,000 | **2,400 ms** | **1,054 MB** | measured |
| 3,176,275 — the wildcards in this list | ≥ 3.1 s | ~10.9 GB | extrapolated |
| 4,629,815 — the plain records in this list | ≥ 4.5 s | ~15.9 GB | extrapolated |
| 7,806,090 — the entire list | ≥ 7.6 s | ~26.8 GB | extrapolated |

At 300,000 patterns — under 10% of the 3.18M wildcards here — Pi-hole already
needs about a gigabyte and takes over two seconds per uncached query.

Memory scales cleanly and predictably at ~3.6 KB per compiled pattern (3.68 at
100k, 3.60 at 300k), so those GB figures should be close. Latency does not: the
per-pattern cost rose from 0.97 µs at 100k to 7.83 µs at 300k. The extrapolated
seconds-per-query figures are therefore derived from the *lower* 100k rate and
should be read as floors — the observed curve is worse than linear, so real
numbers would be higher.

A larger run at 1M patterns was attempted and abandoned: inserting the rows
alone did not complete within ten minutes.

None of this is a defect. The regex table is designed for a handful of
hand-written patterns, where it costs nothing and works well. It is simply not a
bulk import format, so the wildcard portion of a list like this one cannot be
carried across.

## Why this project exists

### Privacy first

This started as a privacy project, and that shows up in the defaults rather than
in options you have to find.

**It resolves queries itself.** With no `FORWARDERS` set, `bancuh-dns` starts a
bundled [unbound](https://nlnetlabs.nl/projects/unbound/) and recurses from the
root servers (`src/main.rs`). The other three engines here are forwarders — they
have no recursor of their own, so out of the box they hand your entire query
stream to whichever public resolver you point them at, commonly Google or
Cloudflare. Blocking ads while sending every lookup to an advertising company is
a strange bargain, and avoiding it is the single biggest reason this exists.

The bundled recursor is configured for privacy, not just function
(`unbound.conf`):

- `qname-minimisation: yes` — send only the labels each authoritative server
  needs, rather than the full name at every step (RFC 7816)
- `auto-trust-anchor-file` — DNSSEC validation on, primed at image build. unbound
  does not validate without an anchor, so leaving this out would fail open
- `hide-identity` / `hide-version` — no server fingerprinting
- `interface: 127.0.0.1` with `access-control: 0.0.0.0/0 refuse` — the recursor
  is reachable only by bancuh-dns, never from outside

**It keeps no record of who asked what.** There are no accounts, no per-client
profiles, and no persistent query log. The optional query log is in memory only,
expires after 10 minutes (`MAX_AGE` in `src/query_log.rs`), is keyed so a visitor
can only ever see queries from their own IP address, and is **off by default**
(`ADMIN_ENABLED`). Nothing survives a restart.

That is a deliberate limitation. Per-user filtering — custom allow/deny lists,
per-device policy — requires identifying every client on every query, which for
plain DNS on port 53 means storing a mapping of people's home IP addresses to
their configuration. That is a record of who queries what, and this project will
not hold one. Users wanting that level of control are better served running their
own instance, which `CONFIG_URL` supports directly.

### The feature intersection

Taken one at a time, every requirement here is met by something off the shelf.
The reason this project exists is that no single engine met all of them at once:

| requirement | bancuh-dns | Pi-hole | Blocky | AdGuard Home |
|---|---|---|---|---|
| 7.8M entries incl. wildcards | yes | no | yes | not in hosts format |
| runs in 1 GB alongside a recursor + TLS front-end | yes | yes | tight | no |
| recurses by default, no third-party forwarder | yes | no | no | no |
| no per-user state or persistent query records | yes | no | configurable | no |
| blocklist config fetched at runtime from a URL | yes | via API/DB | yes | via API |

Pi-hole and AdGuard Home are richer products, with web dashboards, per-client
policy and DHCP that this project has no intention of matching. Blocky is the
closest architectural neighbour and a good choice on hardware with more memory.

### What follows from those constraints

Wildcard support at 7.8M entries, on 1 GB, is what dictated the storage design.
Domains live in RocksDB as sorted keys — plain names and `*.suffix` keys alike —
so a wildcard is an *indexed lookup* rather than a pattern match. A query probes
its own name plus one key per parent label: O(labels), not O(rules).

That is what makes 3.18M wildcards affordable, and it is why the resident set
stays at 41 MB while the 165 MB of data lives on disk and in page cache.

## Method and limitations

- One machine, one run per engine. No repetitions, no confidence intervals.
- Latency measured as wall-clock over a 1,000-query batch through a single
  `dig` process, so it includes `dig` overhead and loopback. Treat the numbers
  as comparable to each other, not as absolute per-query cost.
- Memory is `VmRSS` from `/proc` after the list finished loading and the
  process settled.
- Every engine was given the same hosts/domains list. Engines with a richer
  native syntax (AdGuard Home especially) would do better with a list written
  for them.
- Versions: Pi-hole 6.4.3, Blocky 0.34.0, AdGuard Home latest as of
  2026-09-05, bancuh-dns `:2`.

If you reproduce this and get different results, please open an issue — these
numbers are a snapshot, and all four projects move.
