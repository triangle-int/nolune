#!/usr/bin/env bash
# Sandboxed installer behavior tests: no network, services, or user files.
#
# The installer is a thin wrapper (#127): it downloads the binary, runs
# `nolune onboard`, starts `nolune gateway` in the foreground, and opens the
# browser. Config, tokens, and services are the binary's job, so the mock
# "download" is a fake nolune that records how it was called.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT
mock_bin="$tmp/mock-bin"
mkdir -p "$mock_bin"

cat > "$mock_bin/uname" <<'MOCK'
#!/usr/bin/env bash
case "$1" in
  -s) printf '%s\n' "${MOCK_OS:?}" ;;
  -m) printf '%s\n' "${MOCK_ARCH:?}" ;;
  *) exit 2 ;;
esac
MOCK

cat > "$mock_bin/curl" <<'MOCK'
#!/usr/bin/env bash
set -eu
printf 'curl %s\n' "$*" >> "$MOCK_CALLS"
case "$*" in
  *' -o '*)
    out=''
    while [[ $# -gt 0 ]]; do
      if [[ "$1" = -o ]]; then out=$2; break; fi
      shift
    done
    [[ -n "$out" ]]
    # The "downloaded binary": a fake nolune that behaves like the real CLI contract.
    cat > "$out" <<'BIN'
#!/usr/bin/env bash
set -eu
home="${NOLUNE_HOME:-<unset>}"
case "${1:-}" in
  onboard)
    existed=no
    [[ -f "$home/config.toml" ]] && existed=yes
    printf 'nolune onboard NOLUNE_HOME=%s (config existed: %s)\n' "$home" "$existed" >> "${MOCK_CALLS:?}"
    mkdir -p "$home/bin"
    if [[ "$existed" = no ]]; then
      printf 'auth_token = "mocktokenmocktokenmocktokenmock1"\n# written by mock onboard\n' > "$home/config.toml"
    fi
    printf 'workspace ready at %s\n' "$home"
    ;;
  gateway)
    printf 'nolune gateway NOLUNE_HOME=%s\n' "$home" >> "${MOCK_CALLS:?}"
    if [[ "${MOCK_HEALTHY:-1}" = 1 ]]; then
      printf 'nolune: ready http://localhost:26559\n'
      # A real gateway blocks; the mock returns as if the user stopped it.
      exit 0
    fi
    printf 'port 26559 is already in use\n' >&2
    exit 1
    ;;
  *) printf 'nolune %s\n' "$*" >> "${MOCK_CALLS:?}" ;;
esac
BIN
    ;;
  *'-fsSIL '*)
    # Real chain: latest/download -> /releases/download/<tag>/ -> signed CDN URL.
    printf 'HTTP/2 302 \r\nlocation: https://github.com/triangle-int/nolune/releases/download/v0.33.0/artifact\r\n'
    printf 'HTTP/2 302 \r\nlocation: https://release-assets.githubusercontent.com/github-production-release-asset/1/abc?sig=x%%2By&response-content-disposition=attachment%%3B%%20filename%%3Dartifact\r\n'
    printf 'HTTP/2 200 \r\n'
    ;;
  *'/healthz'*) [[ "${MOCK_HEALTHY:-1}" = 1 ]] ;;
  *) ;;
esac
MOCK

# Service managers must never be touched by the installer any more.
for cmd in launchctl systemctl open xdg-open; do
  cat > "$mock_bin/$cmd" <<'MOCK'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$MOCK_CALLS"
MOCK
done

cat > "$mock_bin/sleep" <<'MOCK'
#!/usr/bin/env bash
exit 0
MOCK
chmod +x "$mock_bin"/*

assert_contains() {
  local file=$1 expected=$2
  if ! grep -Fq -- "$expected" "$file"; then
    printf 'FAIL: missing: %s\n--- %s ---\n' "$expected" "$file" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_exact_call() {
  local file=$1 expected=$2
  if ! grep -Fxq -- "$expected" "$file"; then
    printf 'FAIL: missing exact call: %s\n--- calls ---\n' "$expected" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_absent() {
  local file=$1 unexpected=$2
  if grep -Fq -- "$unexpected" "$file"; then
    printf 'FAIL: unexpected: %s\n--- %s ---\n' "$unexpected" "$file" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_status() {
  local name=$1 expected=$2 actual
  actual=$(cat "$tmp/$name/status")
  if [[ "$actual" -ne "$expected" ]]; then
    printf 'FAIL: %s exited %s, expected %s\n--- output ---\n' "$name" "$actual" "$expected" >&2
    cat "$tmp/$name/output" >&2
    exit 1
  fi
}

# The binary owns config and token. The script must not write config.toml itself,
# and must never echo the token.
assert_binary_owns_config() {
  local name=$1
  local data="$tmp/$name/data"
  assert_contains "$tmp/$name/calls" "nolune onboard NOLUNE_HOME=$data (config existed: no)"
  assert_contains "$data/config.toml" '# written by mock onboard'
  for file in "$tmp/$name/output" "$tmp/$name/calls"; do
    assert_absent "$file" 'mocktokenmocktokenmocktokenmock1'
  done
}

assert_no_service_management() {
  local name=$1
  for verb in launchctl systemctl; do
    assert_absent "$tmp/$name/calls" "$verb "
  done
  assert_absent "$tmp/$name/output" 'setting up service'
  [[ ! -e "$tmp/$name/home/Library/LaunchAgents" ]]
  [[ ! -e "$tmp/$name/home/.config/systemd" ]]
}

run_installer() {
  local name=$1 os=$2 arch=$3 healthy=${4:-1}
  local home="$tmp/$name/home" data="$tmp/$name/data" calls="$tmp/$name/calls"
  mkdir -p "$home"
  : > "$calls"
  set +e
  HOME="$home" NOLUNE_DIR="$data" SHELL=/bin/bash \
    MOCK_OS="$os" MOCK_ARCH="$arch" MOCK_HEALTHY="$healthy" MOCK_CALLS="$calls" \
    PATH="$mock_bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    bash "$root/scripts/install.sh" > "$tmp/$name/output" 2>&1 < /dev/null
  status=$?
  set -e
  printf '%s\n' "$status" > "$tmp/$name/status"
}

# macOS: download, onboard, foreground gateway, browser. No service.
run_installer macos Darwin arm64
assert_status macos 0
assert_binary_owns_config macos
assert_no_service_management macos
assert_contains "$tmp/macos/calls" "nolune gateway NOLUNE_HOME=$tmp/macos/data"
assert_contains "$tmp/macos/calls" '/healthz'
assert_exact_call "$tmp/macos/calls" 'open http://localhost:26559'
assert_contains "$tmp/macos/output" 'nolune gateway install'
assert_contains "$tmp/macos/output" 'http://localhost:26559'

# Fresh installs and their generated updater use the Nolune distribution contract.
assert_contains "$tmp/macos/calls" 'https://github.com/triangle-int/nolune/releases/latest/download/nolune-server-aarch64-apple-darwin'
assert_contains "$tmp/macos/data/bin/update" 'nolune-server-'
if [[ "$(cat "$tmp/macos/data/bin/.version")" != v0.33.0 ]]; then
  printf 'FAIL: recorded version is %q, expected v0.33.0\n' "$(cat "$tmp/macos/data/bin/.version")" >&2
  exit 1
fi
if ! grep -Fq $'downloaded \033[1mv0.33.0\033[0m' "$tmp/macos/output"; then
  echo 'FAIL: installer did not report the resolved release tag' >&2
  exit 1
fi
assert_contains "$tmp/macos/calls" '--connect-timeout 15 --retry 3'
bash -n "$tmp/macos/data/bin/update"
MOCK_CALLS="$tmp/macos/calls" PATH="$mock_bin:$PATH" bash "$tmp/macos/data/bin/update" > "$tmp/macos/update-output"
assert_contains "$tmp/macos/update-output" 'already at v0.33.0'

# Re-running keeps the existing config: onboard sees it and the script does not rewrite it.
run_installer macos Darwin arm64
assert_status macos 0
assert_contains "$tmp/macos/calls" "nolune onboard NOLUNE_HOME=$tmp/macos/data (config existed: yes)"
assert_contains "$tmp/macos/data/config.toml" '# written by mock onboard'
assert_no_service_management macos

# Linux: same flow, xdg-open, no systemd.
run_installer linux Linux x86_64
assert_status linux 0
assert_binary_owns_config linux
assert_no_service_management linux
assert_contains "$tmp/linux/calls" "nolune gateway NOLUNE_HOME=$tmp/linux/data"
assert_exact_call "$tmp/linux/calls" 'xdg-open http://localhost:26559'

# Every platform the release workflow publishes resolves to its own asset.
run_installer macos-intel Darwin x86_64
assert_status macos-intel 0
assert_contains "$tmp/macos-intel/calls" 'https://github.com/triangle-int/nolune/releases/latest/download/nolune-server-x86_64-apple-darwin'
run_installer linux-arm Linux aarch64
assert_status linux-arm 0
assert_contains "$tmp/linux-arm/calls" 'https://github.com/triangle-int/nolune/releases/latest/download/nolune-server-aarch64-unknown-linux-gnu'

# A gateway that never becomes ready fails honestly and does not open a browser.
run_installer unhealthy Darwin arm64 0
[[ $(cat "$tmp/unhealthy/status") -ne 0 ]]
assert_contains "$tmp/unhealthy/calls" 'nolune gateway'
assert_absent "$tmp/unhealthy/calls" 'open http'
assert_contains "$tmp/unhealthy/output" 'did not become ready'
assert_absent "$tmp/unhealthy/output" 'nolune is ready'

# The script contains no config or service logic of its own.
for forbidden in 'auth_token' 'LaunchAgents' 'systemd/user' '<plist' '[Service]' 'launchctl' 'systemctl'; do
  if grep -Fq -- "$forbidden" "$root/scripts/install.sh"; then
    printf 'FAIL: install.sh still contains %q; that belongs to the binary\n' "$forbidden" >&2
    exit 1
  fi
done

printf 'All installer shell tests passed.\n'
