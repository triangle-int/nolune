#!/usr/bin/env bash
# Manual release check for computer use (#21): walk the end-to-end scenario
# against the real Cua Driver on a macOS machine with a graphical session.
#
# The CI scenarios (server/test-support/cua_end_to_end.rs) prove the policy
# against a fake driver and a fake desktop; this is the release check on a
# real window. It checks the prerequisites and exits non-zero when one is
# missing, then prints the steps to verify by hand, in order: list_machines,
# get_window_state on a background window, a verified click, a stale element
# token refused, a denied permission refused before the driver, disconnect
# cleanup. Nothing here drives a window: every observation and action goes
# through the companion, in a conversation, the way a user's would. The only
# commands it runs are `nolune cua status` and, with NOLUNE_TOKEN set, one
# read of the gateway's machine listing.
#
# Usage:
#   scripts/release-check-computer-use.sh
#
# Environment:
#   NOLUNE_BIN    the nolune binary (default: nolune on PATH, else
#                 target/release/nolune, else target/debug/nolune)
#   NOLUNE_HOME   the workspace the gateway runs with (passed through)
#   NOLUNE_URL    the running gateway (default http://localhost:26559)
#   NOLUNE_TOKEN  its API token; with it the listing step is read here
#                 (GET /api/instances/companion/machines), else by hand
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
url=${NOLUNE_URL:-http://localhost:26559}

say() {
    printf '%s\n' "$*"
}

# A missing prerequisite: say what, and exit 2 so a release workflow that
# calls this stops here rather than reading a checklist it cannot walk.
missing() {
    printf 'release-check-computer-use: prerequisite missing: %s\n' "$*" >&2
    exit 2
}

# ── Prerequisites ──────────────────────────────────────────────────────────

say "Computer use release check (docs/computer-use.md, \"Release check\")"
say

[ "$(uname -s)" = "Darwin" ] || missing "this check runs on macOS (uname -s is $(uname -s))"
say "  host: macOS $(sw_vers -productVersion 2>/dev/null || echo '?') on $(uname -m)"

manager=$(launchctl managername 2>/dev/null || true)
[ "$manager" = "Aqua" ] \
    || missing "a graphical login session (launchctl managername reports '${manager:-nothing}', not Aqua): a headless host never starts the driver"
say "  session: Aqua (a graphical login)"

if [ -n "${NOLUNE_BIN:-}" ]; then
    nolune=$NOLUNE_BIN
elif command -v nolune >/dev/null 2>&1; then
    nolune=$(command -v nolune)
elif [ -x "$root/target/release/nolune" ]; then
    nolune=$root/target/release/nolune
elif [ -x "$root/target/debug/nolune" ]; then
    nolune=$root/target/debug/nolune
else
    missing "a nolune binary (set NOLUNE_BIN, put nolune on PATH, or build the server)"
fi
[ -x "$nolune" ] || missing "$nolune is not executable"
say "  nolune: $nolune ($("$nolune" --version 2>/dev/null || echo 'version unknown'))"

# The driver: pinned, installed, healthy, with both grants. `nolune cua
# status` prints exactly these facts and exits non-zero when the version is
# not the pin or the health failed.
status=$("$nolune" cua status 2>&1) || {
    printf '%s\n' "$status" >&2
    missing "\`nolune cua status\` failed (above); run \`nolune cua install\` and grant the driver its permissions"
}
printf '%s\n' "$status" | sed 's/^/    /'
grep -q "matches the pin" <<<"$status" \
    || missing "the running driver's version is not the pin (run \`nolune cua install\`)"
# The workspace install is the shipped path; a driver from NOLUNE_CUA_DRIVER
# or PATH that reports the pinned version still exercises that version.
grep -q "verified against the pin" <<<"$status" \
    || say "  note: no workspace install verified against the pin; the driver above answers. Run \`nolune cua install\` too before a release."
grep -q "health: ok" <<<"$status" \
    || missing "the driver's health is not ok (its failed checks are listed above)"
grep -q "accessibility: granted" <<<"$status" \
    || missing "Accessibility is not granted to CuaDriver.app (System Settings > Privacy & Security > Accessibility)"
grep -q "screen recording: granted" <<<"$status" \
    || missing "Screen Recording is not granted to CuaDriver.app (System Settings > Privacy & Security > Screen & System Audio Recording)"
say "  driver: pinned, verified, healthy, both permissions granted"

# The gateway, when it can be asked: the server-local row is what
# list_machines shows; without a token the first step reads it by hand.
listing=""
if [ -n "${NOLUNE_TOKEN:-}" ]; then
    command -v curl >/dev/null 2>&1 || missing "curl, to read $url with NOLUNE_TOKEN"
    listing=$(curl -fsS -H "Authorization: Bearer $NOLUNE_TOKEN" \
        "$url/api/instances/companion/machines" 2>&1) \
        || missing "the gateway at $url did not answer the machine listing: $listing"
    grep -q '"location":"server_local"' <<<"$listing" \
        || missing "the gateway at $url lists no server-local machine: is it running on this login session with the driver above?"
    say "  gateway: $url lists the server machine (location server_local)"
else
    say "  gateway: not asked (set NOLUNE_TOKEN to read the machine listing here)"
fi
say
say "Prerequisites hold. Verify the following by hand, in order, in a chat with"
say "the companion at $url. Every step names the refusal or the result to see;"
say "a step that does not read as written fails the release."
say

# ── The checklist ──────────────────────────────────────────────────────────

step() {
    say "$1"
    shift
    for line in "$@"; do
        say "    $line"
    done
    say
}

step "1. list_machines" \
    "Choose the server home in the composer and ask: which computers can you use?" \
    "Expect: list_machines lists this machine with location server_local, driver" \
    "version $(grep -o 'pinned: [0-9.]*' <<<"$status" | head -1 | cut -d' ' -f2), health healthy and both permissions granted; the trail line" \
    "reads \"listing computers\". The Computers page shows the same row, online." \
    "Refusal to see first: with nothing chosen in the composer and a desktop app" \
    "also connected with a driver, the same question must end in" \
    "choose_a_computer, naming both, never a pick."

step "2. get_window_state on a background window" \
    "Open TextEdit with a new document that says \"release check\", then put another" \
    "app's window fully in front of it and keep that one focused. Ask: observe the" \
    "TextEdit window, without a screenshot." \
    "Expect: discover_windows finds it; get_window_state returns a snapshot_id and" \
    "the accessibility elements of the document, pixel_addresses.allowed is false," \
    "no image is shown, and TextEdit stays behind: the front window never moves" \
    "and focus never changes. The trail reads \"observing a window on the server" \
    "home\". Then ask for the same with a screenshot: one capture is taken, shown" \
    "beside the elements and saved among the uploads (the result names it), and" \
    "still nothing comes to the front."

step "3. a verified click" \
    "Ask: click the document's text area and check that the window still exists." \
    "Expect: act goes out addressed by an element_token in the background (the" \
    "window stays behind), the verification the model asked for runs right after" \
    "it, and the result reports verified: true; the trail reads \"click on the" \
    "server home\". Refusal to see first: ask for a click by coordinates (a point)" \
    "on the healthy window and expect pixel_refused; nothing reaches the driver."

step "4. a stale element token refused" \
    "Ask: observe the TextEdit window again, then act with the element_token the" \
    "first observation issued." \
    "Expect: stale_snapshot naming the newer snapshot and the one it replaced," \
    "before anything is sent; a token from the latest observation goes through," \
    "and the next action on that window needs a fresh observation" \
    "(snapshot_consumed)."

step "5. a denied permission refused before the driver" \
    "In System Settings > Privacy & Security > Accessibility, turn CuaDriver off." \
    "Run \`nolune cua status\`: health degraded, accessibility: denied. Ask for a" \
    "click in the TextEdit window." \
    "Expect: permission_denied naming Accessibility and how to grant it, from the" \
    "descriptor, with no session opened and nothing sent to the driver; the" \
    "Computers page shows the grant as denied. Turn CuaDriver back on and" \
    "confirm \`nolune cua status\` reports health ok again."

step "6. the desktop app installs its own driver" \
    "On a Mac with no driver (move ~/.nolune/cua-driver aside and quit CuaDriver)," \
    "open the Nolune desktop app's Settings > Computer use. Expect: the status" \
    "says no driver is installed and offers Install driver; no line anywhere on" \
    "that page names a terminal command. Press it." \
    "Expect: the steps are narrated (downloading with the pinned size, checksum" \
    "matches, the driver reports the pinned version), then the outcome names" \
    "where it landed and asks for the two grants. Grant them from the same page:" \
    "the macOS prompts and the System Settings entries name CuaDriver" \
    "(com.trycua.driver), never Nolune. Refresh: the driver reports the pin and" \
    "health ok. Within a few seconds the companion sees the computer with a" \
    "driver (list_machines and the Computers page), without reconnecting by hand," \
    "and an action on one of its windows now runs."

step "7. disconnect cleanup" \
    "Stop the gateway (Ctrl-C, or \`nolune gateway stop\` for the service)." \
    "Expect: the log ends the open sessions, unregisters the target and stops the" \
    "driver child before connections drain; after a restart the row is back." \
    "With a desktop app connected as a second target: quit it while an action is" \
    "waiting on it. Expect: the call fails at once as a retryable" \
    "runtime_unavailable, the desktop's target leaves list_machines and the" \
    "Computers page shows its row offline with the driver fields cleared."

say "Privacy, at every step: the only capture is the one-shot window screenshot of"
say "step 2; there is no recording, no frame stream and no watcher of the screen,"
say "before, between or after the steps (docs/computer-use.md, \"Privacy\")."
