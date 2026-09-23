#!/usr/bin/env bash
set -euo pipefail

release_id() {
  gh release view "${RELEASE_TAG:?}" \
    --repo "${GH_REPO:?}" \
    --json databaseId \
    --jq .databaseId
}

case "${1:?Expected release, publish or nightly}" in
  release)
    error=$(mktemp)
    trap 'rm -f "$error"' EXIT
    if release_id > /dev/null 2> "$error"; then
      echo "Reusing release $RELEASE_TAG"
    elif grep -Fxq 'release not found' "$error"; then
      notes_file=${RELEASE_NOTES_FILE:-/tmp/release-notes.md}
      # A concurrent run may have created the release after our lookup.
      if ! gh release create "$RELEASE_TAG" \
        --repo "$GH_REPO" \
        --title "Nolune $RELEASE_TAG" \
        --notes-file "$notes_file" \
        --draft; then
        release_id > /dev/null
      fi
    else
      cat "$error" >&2
      echo 'Cannot determine whether the release exists' >&2
      exit 1
    fi
    ;;
  publish)
    id=$(release_id)
    gh api --method PATCH "repos/${GH_REPO}/releases/${id}" \
      -F draft=false -f make_latest=legacy
    ;;
  nightly)
    # Refresh the rolling `nightly` prerelease from the assets in $2. Clients
    # tell builds apart by the body, which must stay exactly this string
    # (server/src/routes/update.rs, scripts/install.sh).
    dir=${2:?Expected the directory of nightly assets}
    notes="Auto-built from main (${SHA:0:7})"
    title="Nightly $(date -u +%Y-%m-%d)"
    existing=$(mktemp)
    error=$(mktemp)
    trap 'rm -f "$existing" "$error"' EXIT
    if gh release view nightly --repo "${GH_REPO:?}" --json assets --jq '.assets[].name' > "$existing" 2> "$error"; then
      # Binaries first, body second: a client that reads the new commit always
      # finds its binary.
      gh release upload nightly "$dir"/* --repo "$GH_REPO" --clobber
      while read -r name; do
        [[ -z "$name" || -e "$dir/$name" ]] || gh release delete-asset nightly "$name" --repo "$GH_REPO" --yes
      done < "$existing"
      gh release edit nightly --repo "$GH_REPO" \
        --title "$title" --notes "$notes" --prerelease --latest=false
      # Clients read the body, not the tag, so moving the tag is cosmetic.
      gh api --method PATCH "repos/${GH_REPO}/git/refs/tags/nightly" \
        -f sha="${SHA:?}" -F force=true > /dev/null ||
        echo '::warning::Could not move the nightly tag; the release still serves this build'
    elif grep -Fxq 'release not found' "$error"; then
      gh release create nightly "$dir"/* --repo "$GH_REPO" --target "${SHA:?}" \
        --title "$title" --notes "$notes" --prerelease --latest=false
    else
      cat "$error" >&2
      echo 'Cannot determine whether the nightly release exists' >&2
      exit 1
    fi
    ;;
  *) exit 2 ;;
esac
