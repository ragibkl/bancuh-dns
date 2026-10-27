#!/bin/sh
# Smoke test for a built image: start it with the tiny config in ./config and
# check real DNS answers over UDP and TCP.
#
# Usage: tests/smoke/run.sh <image>
# Needs docker and dig on the host. Exits non-zero on the first failure.
set -eu

IMAGE=${1:?usage: $0 <image>}
DIR=$(cd "$(dirname "$0")" && pwd)
NAME=bancuh-dns-smoke
PORT=15353

cleanup() {
    status=$?
    if [ "$status" -ne 0 ]; then
        echo "--- container logs ---"
        docker logs "$NAME" 2>&1 | tail -50 || true
    fi
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT

docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" \
    -v "$DIR/config:/config:ro" \
    -e CONFIG_URL=/config/configuration.yaml \
    -p "127.0.0.1:$PORT:53/udp" -p "127.0.0.1:$PORT:53/tcp" \
    "$IMAGE" >/dev/null

q() { # q <name> [type] [extra dig options...]
    name=$1
    shift
    type=${1:-A}
    [ $# -gt 0 ] && shift
    dig @127.0.0.1 -p "$PORT" +short +time=2 +tries=1 "$@" "$name" "$type"
}

# Listeners stay closed until the first blocklist is compiled and loaded.
echo "waiting for the server"
i=0
until q ads.example.com A | grep -qx '0.0.0.0'; do
    i=$((i + 1))
    if [ "$i" -ge 60 ]; then
        echo "FAIL: no filtered answer after 120 s"
        exit 1
    fi
    sleep 2
done
echo "ready after about $((i * 2)) s"

fail=0
check() { # check <description> <expected-grep-pattern> <dig args...>
    desc=$1
    pattern=$2
    shift 2
    out=$(q "$@" || true)
    if printf '%s\n' "$out" | grep -qE "$pattern"; then
        echo "ok    $desc"
    else
        echo "FAIL  $desc: got '$(printf '%s' "$out" | tr '\n' ' ')'"
        fail=1
    fi
}

# A real IPv4 address: first octet not 0, so a blocked 0.0.0.0 never matches.
IPV4='^[1-9][0-9]*\.[0-9]+\.[0-9]+\.[0-9]+$'

check "hosts-format entry blocked (A)" '^0\.0\.0\.0$' ads.example.com A
check "hosts-format entry blocked (AAAA)" '^::$' ads.example.com AAAA
check "127.0.0.1 hosts entry blocked too" '^0\.0\.0\.0$' tracker.example.org A
check "domains-format entry blocked" '^0\.0\.0\.0$' blocked.example.org A
check "blocked over TCP" '^0\.0\.0\.0$' blocked.example.org A +tcp
check "whitelist beats blacklist" "$IPV4" example.net A
check "CNAME override applied" '^example\.com\.$' rewrite.example.org A
check "unlisted name resolves upstream" "$IPV4" example.com A
check "unlisted name resolves over TCP" "$IPV4" example.com A +tcp

exit "$fail"
