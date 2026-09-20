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

python3 - "$root/.github/workflows/release.yml" "$root/.github/workflows/ci.yml" <<'CHECK'
import re
import sys
from pathlib import Path
workflow = Path(sys.argv[1]).read_text()
ci = Path(sys.argv[2]).read_text()
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
CHECK

printf 'All release workflow shell tests passed.\n'
