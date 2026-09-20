#!/usr/bin/env bash
# Fetch and verify one pinned Cua Driver release asset (#20).
#
# The release workflow runs this for every desktop target so a Nolune release
# ships the exact driver bytes it was verified against. The pin (version,
# tag, per-target asset, sha256, size) is read from cua-protocol/cua-driver.pin,
# the shell-readable copy of cua_protocol::cua_driver_pin that the cua-protocol
# tests keep in step with the Rust table. Nothing here installs, runs, or
# updates a driver, and no file is kept until its size and sha256 match.
#
# Usage:
#   scripts/cua-driver.sh pin <target triple> [--pin-file <file>]
#       Print "<asset name> <sha256> <size>" for the target.
#   scripts/cua-driver.sh fetch --target <triple> --dest <dir>
#                              [--base-url <url>] [--pin-file <file>]
#       Download <base-url>/<asset name> (default: the pinned GitHub release),
#       verify it, and keep it as <dir>/<asset name> beside <asset name>.sha256.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
pin_file="$root/cua-protocol/cua-driver.pin"

fail() {
    printf 'cua-driver.sh: %s\n' "$*" >&2
    exit 1
}

# A pin fact by key, e.g. `pin_fact tag`.
pin_fact() {
    awk -v key="$1" '$1 == key { print $2; exit }' "$pin_file"
}

# The asset line for a target: "<name> <sha256> <size>".
pin_asset() {
    awk -v target="$1" '$1 == "asset" && $2 == target { print $3, $4, $5; exit }' "$pin_file"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

mode=${1:-}
[[ -n "$mode" ]] || fail 'expected pin or fetch'
shift

target='' dest='' base_url=''
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) target=${2:?--target needs a triple}; shift 2 ;;
        --dest) dest=${2:?--dest needs a directory}; shift 2 ;;
        --base-url) base_url=${2:?--base-url needs a URL}; shift 2 ;;
        --pin-file) pin_file=${2:?--pin-file needs a file}; shift 2 ;;
        -*) fail "unknown option $1" ;;
        *)
            [[ -z "$target" ]] || fail "unexpected argument $1"
            target=$1; shift ;;
    esac
done
[[ -f "$pin_file" ]] || fail "pin file $pin_file does not exist"
[[ -n "$target" ]] || fail 'a target triple is required'

line=$(pin_asset "$target")
[[ -n "$line" ]] || fail "no Cua Driver is pinned for $target (see $pin_file)"
read -r asset sha256 size <<< "$line"

case "$mode" in
    pin)
        printf '%s %s %s\n' "$asset" "$sha256" "$size"
        ;;
    fetch)
        [[ -n "$dest" ]] || fail '--dest is required'
        if [[ -z "$base_url" ]]; then
            base_url="https://github.com/$(pin_fact repository)/releases/download/$(pin_fact tag)"
        fi
        url="${base_url%/}/$asset"
        mkdir -p "$dest"
        tmp=$(mktemp "$dest/.$asset.XXXXXX")
        trap 'rm -f "$tmp"' EXIT
        printf 'fetching %s (%s bytes)\n' "$url" "$size"
        curl -fL --connect-timeout 15 --retry 3 --retry-delay 2 --retry-connrefused "$url" -o "$tmp" \
            || fail "download failed: $url"
        actual_size=$(wc -c < "$tmp" | tr -d ' ')
        [[ "$actual_size" == "$size" ]] \
            || fail "$asset is $actual_size bytes, the pin expects $size (sha256 $sha256); nothing was kept"
        actual_sha256=$(sha256_of "$tmp")
        [[ "$actual_sha256" == "$sha256" ]] \
            || fail "$asset sha256 mismatch: expected $sha256, got $actual_sha256; nothing was kept"
        mv "$tmp" "$dest/$asset"
        trap - EXIT
        printf '%s  %s\n' "$sha256" "$asset" > "$dest/$asset.sha256"
        printf 'verified %s sha256 %s (%s bytes)\n' "$asset" "$sha256" "$size"
        ;;
    *)
        fail "unknown mode $mode (expected pin or fetch)"
        ;;
esac
