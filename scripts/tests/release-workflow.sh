#!/usr/bin/env bash
# Deterministic tests: no network or GitHub access.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export PATH="$tmp:$PATH"
export CALLS="$tmp/calls" COMMITS_FILE="$tmp/commits" RELEASE_NOTES_FILE="$tmp/release-notes.md"
export GH_REPO=triangle-int/nolune RELEASE_TAG=v1.2.3
printf 'abc123 Test change\n' > "$COMMITS_FILE"
printf 'Generated release notes\n\n## Bug fixes\n- Fixed startup.\n' > "$RELEASE_NOTES_FILE"

cat > "$tmp/gh" <<'MOCK'
#!/usr/bin/env bash
set -eu
printf '%s\n' "$*" >> "$CALLS"
case "$1 $2" in
  'release view')
    case "$SCENARIO" in
      draft|published|success|publish_error) echo 123 ;;
      race)
        if [[ $(wc -l < "$CALLS") -eq 1 ]]; then
          echo 'release not found' >&2
          exit 1
        fi
        echo 123 ;;
      absent|create_error) echo 'release not found' >&2; exit 1 ;;
      lookup_error) echo 'network unavailable' >&2; exit 1 ;;
      *) exit 99 ;;
    esac ;;
  'release create')
    [[ "$*" == $'release create v1.2.3 --repo triangle-int/nolune --title Nolune v1.2.3 --notes-file '* ]]
    [[ $(cat "${@: -2:1}") == $'Generated release notes\n\n## Bug fixes\n- Fixed startup.' ]]
    [[ "$SCENARIO" == absent ]] ;;
  'api --method')
    [[ "$*" == 'api --method PATCH repos/triangle-int/nolune/releases/123 -F draft=false -f make_latest=legacy' ]]
    [[ "$SCENARIO" != publish_error ]] ;;
  *) exit 99 ;;
esac
MOCK
chmod +x "$tmp/gh"

run() {
  local mode scenario expected status
  mode=$1
  scenario=$2
  expected=$3
  status=0
  export SCENARIO=$scenario
  : > "$CALLS"
  bash "$root/scripts/release-workflow.sh" "$mode" > "$tmp/log" 2>&1 || status=$?
  if { [ "$expected" = success ] && [ "$status" -ne 0 ]; } ||
     { [ "$expected" = failure ] && [ "$status" -eq 0 ]; }; then
    cat "$tmp/log" >&2
    echo "FAIL: $mode $scenario ($status)" >&2
    exit 1
  fi
}

for scenario in draft published; do
  run release "$scenario" success
  [[ $(wc -l < "$CALLS") -eq 1 ]]
  if grep -q 'release create' "$CALLS"; then
    echo "FAIL: existing $scenario release triggered create" >&2
    exit 1
  fi
done
run release absent success
[[ $(grep -c '^release create ' "$CALLS") -eq 1 ]]
run release race success
[[ $(wc -l < "$CALLS") -eq 3 ]]
run release create_error failure
run release lookup_error failure
if grep -q 'release create' "$CALLS"; then
  echo 'FAIL: lookup error triggered create' >&2
  exit 1
fi

run publish success success
[[ $(cat "$CALLS") == "$(printf '%s\n' \
  'release view v1.2.3 --repo triangle-int/nolune --json databaseId --jq .databaseId' \
  'api --method PATCH repos/triangle-int/nolune/releases/123 -F draft=false -f make_latest=legacy')" ]]
run publish lookup_error failure
[[ $(wc -l < "$CALLS") -eq 1 ]]
run publish publish_error failure
[[ $(wc -l < "$CALLS") -eq 2 ]]

# ── scripts/cua-driver.sh: the pinned Cua Driver fetch the desktop job runs (#20) ──
# curl is a mock that copies a local file; the pin file under test carries the
# digest of that file, so a matching download passes and a tampered one fails
# before anything is kept.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}
assets="$tmp/assets"
mkdir -p "$assets"
export MOCK_ASSETS="$assets"
cat > "$tmp/curl" <<'MOCK'
#!/usr/bin/env bash
set -eu
printf 'curl %s\n' "$*" >> "$CALLS"
out='' url=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) out=$2; shift 2 ;;
    http*) url=$1; shift ;;
    *) shift ;;
  esac
done
[[ -n "$out" && -n "$url" ]]
src="$MOCK_ASSETS/$(basename "$url")"
[[ -f "$src" ]] || exit 22
cp "$src" "$out"
MOCK
chmod +x "$tmp/curl"
good_asset='cua-driver-rs-9.9.9-darwin-universal.tar.gz'
linux_asset='cua-driver-rs-9.9.9-linux-x86_64-binary.tar.gz'
printf 'pretend this is a driver tarball\n' > "$assets/$good_asset"
# Same size, one byte flipped: only the digest can tell.
printf 'pretend this is a driver tarbalL\n' > "$assets/$linux_asset"
good_sha=$(sha256_of "$assets/$good_asset")
good_size=$(wc -c < "$assets/$good_asset" | tr -d ' ')
pin="$tmp/test.pin"
cat > "$pin" <<PIN
# test pin
version 9.9.9
tag cua-driver-rs-v9.9.9
repository trycua/cua
commit 0000000000000000000000000000000000000000
asset aarch64-apple-darwin $good_asset $good_sha $good_size
asset x86_64-unknown-linux-gnu $linux_asset $good_sha $good_size
PIN

[[ $(bash "$root/scripts/cua-driver.sh" pin aarch64-apple-darwin --pin-file "$pin") == "$good_asset $good_sha $good_size" ]]

# A matching download is kept beside its checksum line and came from the pinned release URL.
dest="$tmp/sidecar-ok"
: > "$CALLS"
bash "$root/scripts/cua-driver.sh" fetch --target aarch64-apple-darwin --dest "$dest" --pin-file "$pin" > "$tmp/log" 2>&1 || { cat "$tmp/log" >&2; echo 'FAIL: verified fetch failed' >&2; exit 1; }
[[ -f "$dest/$good_asset" ]]
cmp -s "$dest/$good_asset" "$assets/$good_asset"
[[ $(cat "$dest/$good_asset.sha256") == "$good_sha  $good_asset" ]]
grep -Fq -- "https://github.com/trycua/cua/releases/download/cua-driver-rs-v9.9.9/$good_asset" "$CALLS"
grep -Fq -- '--connect-timeout' "$CALLS"
# A stalled or endless mirror fails instead of hanging the release job or filling its disk.
grep -Fq -- '--max-time' "$CALLS"
grep -Fq -- "--max-filesize $good_size" "$CALLS"
grep -Fq -- "$good_sha" "$tmp/log"
[[ $(ls -A "$dest" | wc -l | tr -d ' ') -eq 2 ]]

# A tampered download is refused, names both digests, and leaves nothing behind.
dest="$tmp/sidecar-bad"
if bash "$root/scripts/cua-driver.sh" fetch --target x86_64-unknown-linux-gnu --dest "$dest" --pin-file "$pin" > "$tmp/log" 2>&1; then
  echo 'FAIL: a wrong checksum must fail the fetch' >&2
  exit 1
fi
grep -Fq -- "$good_sha" "$tmp/log"
grep -Fq -- "$(sha256_of "$assets/$linux_asset")" "$tmp/log"
[[ ! -e "$dest/$linux_asset" ]]
[[ ! -e "$dest/$linux_asset.sha256" ]]
[[ -z "$(ls -A "$dest" 2>/dev/null)" ]]

# A truncated download fails on its size before any digest is computed.
printf 'pretend' > "$assets/$linux_asset"
dest="$tmp/sidecar-short"
if bash "$root/scripts/cua-driver.sh" fetch --target x86_64-unknown-linux-gnu --dest "$dest" --pin-file "$pin" > "$tmp/log" 2>&1; then
  echo 'FAIL: a truncated download must fail the fetch' >&2
  exit 1
fi
grep -Fq -- "expects $good_size" "$tmp/log"
[[ -z "$(ls -A "$dest" 2>/dev/null)" ]]

# A target the pin does not cover, and a mirror override.
if bash "$root/scripts/cua-driver.sh" fetch --target riscv64gc-unknown-linux-gnu --dest "$tmp/none" --pin-file "$pin" > "$tmp/log" 2>&1; then
  echo 'FAIL: an unpinned target must fail' >&2
  exit 1
fi
grep -Fq 'riscv64gc-unknown-linux-gnu' "$tmp/log"
[[ ! -e "$tmp/none" ]]
: > "$CALLS"
bash "$root/scripts/cua-driver.sh" fetch --target aarch64-apple-darwin --dest "$tmp/mirror" --pin-file "$pin" --base-url 'https://mirror.example/cua/' > "$tmp/log" 2>&1
grep -Fq -- "https://mirror.example/cua/$good_asset" "$CALLS"

# The committed pin resolves every release target to a real asset name and digest.
for triple in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-msvc; do
  line=$(bash "$root/scripts/cua-driver.sh" pin "$triple")
  [[ "$line" =~ ^cua-driver-rs-[0-9.]+-[a-z0-9_-]+\.(tar\.gz|zip)\ [0-9a-f]{64}\ [0-9]+$ ]] || { echo "FAIL: pin $triple -> $line" >&2; exit 1; }
done

python3 - "$root/.github/workflows/release.yml" "$root/.github/workflows/ci.yml" "$root/cua-protocol/cua-driver.pin" <<'CHECK'
import re
import sys
from pathlib import Path
workflow = Path(sys.argv[1]).read_text()
ci = Path(sys.argv[2]).read_text()
pin = Path(sys.argv[3]).read_text()
jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", workflow, re.M | re.S))
for required_job in ("create-release", "server", "desktop", "publish"):
    assert required_job in jobs
assert "docker" not in jobs
publish = jobs["publish"]
assert "    needs: [server, desktop]" in publish
assert "      contents: write" in publish
assert "packages: write" not in workflow
assert "docker" not in workflow.lower()
assert "ghcr" not in workflow.lower()
assert workflow.count("scripts/release-workflow.sh publish") == 1
create_release = jobs["create-release"]
assert "python3 scripts/release_notes.py" in create_release
assert "--release-tag \"$RELEASE_TAG\"" in create_release
assert "OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}" in create_release
assert "RELEASE_NOTES_MODEL: ${{ vars.RELEASE_NOTES_MODEL }}" in create_release
assert "RELEASE_NOTES_FILE: /tmp/release-notes.md" in create_release
assert "pull-requests: read" in workflow
assert '--exclude-contributor "${{ github.actor }}"' in create_release
assert create_release.index("python3 scripts/release_notes.py") < create_release.index("scripts/release-workflow.sh release")
assert "python3 scripts/tests/test_release_notes.py" in ci

# The desktop job fetches and verifies the pinned Cua Driver for its target
# before building, and only a real release uploads it (#20).
desktop = jobs["desktop"]
pinned_targets = {line.split()[1] for line in pin.splitlines() if line.startswith("asset ")}
desktop_targets = re.findall(r"^\s+target: (\S+)$", desktop, re.M)
assert len(desktop_targets) == desktop.count("- platform:"), "every desktop matrix entry names its target"
assert set(desktop_targets) <= pinned_targets, (desktop_targets, pinned_targets)
fetch = "bash scripts/cua-driver.sh fetch --target ${{ matrix.target }}"
assert desktop.count(fetch) == 1
assert desktop.index(fetch) < desktop.index("tauri-apps/tauri-action"), "fetch and verify before the build"
steps = desktop.split("      - name: ")
fetch_step = next(step for step in steps if fetch in step)
assert "cua-driver-dist" in fetch_step
upload_step = next(step for step in steps if "gh release upload" in step and "cua-driver-dist" in step)
assert "if: inputs.dry_run != true" in upload_step, "only a real release uploads the driver"
assert steps.index(fetch_step) < steps.index(upload_step)
CHECK

printf 'All release workflow shell tests passed.\n'
