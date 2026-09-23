#!/usr/bin/env bash
# One command for local development: the server plus the web client with hot
# reload, against a throwaway workspace inside the checkout.
#
# Usage:
#   ./scripts/dev.sh            # start (installs client deps on first run)
#   ./scripts/dev.sh --fresh    # wipe the dev workspace first
#
# Environment:
#   NOLUNE_DEV_PORT    Server port (default 26560, so an installed Nolune on
#                      26559 keeps running beside it)
#   NOLUNE_DEV_HOME    Dev workspace (default <repo>/.dev/home); never ~/.nolune
#   ANTHROPIC_API_KEY, OPENAI_API_KEY, OPENROUTER_API_KEY, ...
#                      Read from the shell or from <repo>/.env
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
port=${NOLUNE_DEV_PORT:-26560}
home=${NOLUNE_DEV_HOME:-$root/.dev/home}
client_port=5173

say()  { printf '\033[36m▸\033[0m %s\n' "$*"; }
fail() { printf '\033[31m✗\033[0m %s\n' "$*" >&2; exit 1; }

for arg in "$@"; do
  case "$arg" in
    --fresh) fresh=1 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) fail "unknown option: $arg (try --help)" ;;
  esac
done

# ── Tools ────────────────────────────────────────────────────────────────────
# rustup reads rust-toolchain.toml, so any rustup install builds with the pinned
# Rust. pnpm comes from corepack, pinned by client/package.json.
command -v cargo >/dev/null || fail "Rust is missing: install rustup from https://rustup.rs"
command -v node >/dev/null || fail "Node.js is missing: install Node 22 LTS from https://nodejs.org"
if ! command -v pnpm >/dev/null; then
  say "enabling pnpm through corepack"
  corepack enable pnpm 2>/dev/null || fail "pnpm is missing: run 'corepack enable pnpm' or 'npm i -g pnpm'"
fi

# ── Workspace ────────────────────────────────────────────────────────────────
if [[ -n "${fresh:-}" ]]; then
  say "wiping $home"
  rm -rf "$home"
fi
mkdir -p "$home"
if [[ ! -f "$home/config.toml" ]]; then
  # Loopback only and no token, so no pairing: the same setup
  # server/config.example.toml documents for a server nobody else can reach.
  cat > "$home/config.toml" <<EOF
host = "127.0.0.1"
port = $port
auth_token = ""
EOF
  say "created dev workspace at $home"
fi

if [[ -f "$root/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  . "$root/.env"
  set +a
fi

# ── Client deps ──────────────────────────────────────────────────────────────
if [[ ! -d "$root/client/node_modules" || "$root/client/pnpm-lock.yaml" -nt "$root/client/node_modules/.modules.yaml" ]]; then
  say "installing client dependencies"
  pnpm --dir "$root/client" install --frozen-lockfile
fi

# ── Run ──────────────────────────────────────────────────────────────────────
pids=()
cleanup() {
  trap - EXIT INT TERM
  # cargo and pnpm each run a child (nolune, vite) that outlives its parent's death.
  for pid in "${pids[@]}"; do
    pkill -TERM -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

say "building and starting the server on :$port (first build takes a few minutes)"
(cd "$root" && NOLUNE_HOME="$home" PORT="$port" exec cargo run -p server -- gateway) &
pids+=($!)

(cd "$root/client" && NOLUNE_DEV_PORT="$port" exec pnpm dev --port "$client_port" --strictPort) &
pids+=($!)

say "open http://localhost:$client_port once the server prints 'nolune: ready'"
say "Ctrl-C stops both"

# Whichever exits first takes the other down with it.
while :; do
  for pid in "${pids[@]}"; do
    kill -0 "$pid" 2>/dev/null || exit 1
  done
  sleep 1
done
