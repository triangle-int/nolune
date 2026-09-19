#!/usr/bin/env bash
# Sandboxed uninstaller tests: the script delegates to `nolune uninstall` and only
# keeps the shell-specific parts (the prompt and the PATH cleanup).
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
  *) exit 2 ;;
esac
MOCK
for cmd in launchctl systemctl pgrep; do
  cat > "$mock_bin/$cmd" <<'MOCK'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$MOCK_CALLS"
exit 1
MOCK
done
chmod +x "$mock_bin"/*

assert_contains() {
  local file=$1 expected=$2
  if ! grep -Fq -- "$expected" "$file"; then
    printf 'FAIL: missing: %s\n--- %s ---\n' "$expected" "$file" >&2
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

# Seed an install: fake binary that records how uninstall was invoked and honours it.
seed() {
  local name=$1 with_binary=${2:-1}
  local home="$tmp/$name/home" data="$tmp/$name/data"
  mkdir -p "$home" "$data/bin" "$data/instances"
  printf 'auth_token = "x"\n' > "$data/config.toml"
  printf '\n# nolune\nexport PATH="%s/bin:$PATH"\n' "$data" > "$home/.bashrc"
  if [[ "$with_binary" = 1 ]]; then
    cat > "$data/bin/nolune" <<'BIN'
#!/usr/bin/env bash
set -eu
printf 'nolune %s NOLUNE_HOME=%s\n' "$*" "${NOLUNE_HOME:-<unset>}" >> "${MOCK_CALLS:?}"
[[ "${1:-}" = uninstall ]] || exit 2
for a in "$@"; do [[ $a = --help ]] && exit 0; done
keep=0; yes=0
for a in "$@"; do [[ $a = --keep-data ]] && keep=1; [[ $a = --yes || $a = -y ]] && yes=1; done
if [[ $keep = 1 ]]; then rm -rf "$NOLUNE_HOME/bin"; elif [[ $yes = 1 ]]; then rm -rf "$NOLUNE_HOME"; else echo 'refusing without --yes' >&2; exit 1; fi
BIN
    chmod +x "$data/bin/nolune"
  fi
}

run_uninstaller() {
  local name=$1 keep=${2:-}
  local home="$tmp/$name/home" data="$tmp/$name/data" calls="$tmp/$name/calls"
  : > "$calls"
  set +e
  HOME="$home" NOLUNE_DIR="$data" SHELL=/bin/bash KEEP_DATA="$keep" \
    MOCK_OS=Darwin MOCK_CALLS="$calls" \
    PATH="$mock_bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    bash "$root/scripts/uninstall.sh" > "$tmp/$name/output" 2>&1 < /dev/null
  printf '%s\n' "$?" > "$tmp/$name/status"
  set -e
}

# KEEP_DATA=1 delegates with --keep-data; data survives; PATH entry is cleaned.
seed keep
run_uninstaller keep 1
[[ $(cat "$tmp/keep/status") -eq 0 ]]
assert_contains "$tmp/keep/calls" "nolune uninstall --keep-data --yes NOLUNE_HOME=$tmp/keep/data"
[[ -f "$tmp/keep/data/config.toml" ]]
[[ ! -e "$tmp/keep/data/bin" ]]
assert_absent "$tmp/keep/home/.bashrc" 'nolune'
assert_absent "$tmp/keep/calls" 'launchctl'
assert_absent "$tmp/keep/calls" 'systemctl'

# No terminal and no KEEP_DATA: the safe default keeps data.
seed default
run_uninstaller default
[[ $(cat "$tmp/default/status") -eq 0 ]]
assert_contains "$tmp/default/calls" 'nolune uninstall --keep-data --yes'
[[ -f "$tmp/default/data/config.toml" ]]

# KEEP_DATA=0 removes everything through the binary.
seed full
run_uninstaller full 0
[[ $(cat "$tmp/full/status") -eq 0 ]]
assert_contains "$tmp/full/calls" "nolune uninstall --yes NOLUNE_HOME=$tmp/full/data"
assert_absent "$tmp/full/calls" '--keep-data'
[[ ! -e "$tmp/full/data" ]]

# An install that predates the binary's uninstall command is still cleaned up.
seed legacy 0
run_uninstaller legacy 0
[[ $(cat "$tmp/legacy/status") -eq 0 ]]
[[ ! -e "$tmp/legacy/data" ]]
assert_absent "$tmp/legacy/home/.bashrc" 'nolune'

# The script contains no service logic of its own.
for forbidden in 'launchctl' 'systemctl' 'LaunchAgents' 'systemd/user'; do
  if grep -Fq -- "$forbidden" "$root/scripts/uninstall.sh"; then
    printf 'FAIL: uninstall.sh still contains %q; that belongs to the binary\n' "$forbidden" >&2
    exit 1
  fi
done

printf 'All uninstaller shell tests passed.\n'
