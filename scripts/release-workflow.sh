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
    suffix=.nightly-upload
    assets=$(mktemp)
    error=$(mktemp)
    trap 'rm -f "$assets" "$error"' EXIT

    # Only the id comes from the tag lookup: GitHub serves /releases/tags/<tag>
    # (and so `gh release view/upload/edit`) with a stale asset list for a while
    # after assets change, so every other call addresses the release by id.
    if id=$(gh release view nightly --repo "${GH_REPO:?}" --json databaseId --jq .databaseId 2> "$error"); then
      delete_asset() {
        gh api --method DELETE "repos/$GH_REPO/releases/assets/$1" < /dev/null 2> "$error" ||
          grep -Fq 'HTTP 404' "$error" || { cat "$error" >&2; exit 1; }
      }
      gh api --paginate "repos/$GH_REPO/releases/$id/assets" \
        --jq '.[] | "\(.id)\t\(.name)"' > "$assets"

      # An upload left behind by a failed run would block this one.
      while IFS=$'\t' read -r asset name; do
        [[ "$name" != *"$suffix" ]] || delete_asset "$asset"
      done < "$assets"

      # Upload under a temporary name, then swap it in: the old binary serves
      # downloads until the new one takes its name.
      for file in "$dir"/*; do
        name=$(basename "$file")
        new=$(gh api --method POST \
          "https://uploads.github.com/repos/$GH_REPO/releases/$id/assets?name=$name$suffix" \
          -H 'Content-Type: application/octet-stream' --input "$file" --jq .id < /dev/null)
        while IFS=$'\t' read -r asset existing; do
          [[ "$existing" != "$name" ]] || delete_asset "$asset"
        done < "$assets"
        gh api --method PATCH "repos/$GH_REPO/releases/assets/$new" -f name="$name" < /dev/null > /dev/null
      done

      # Drop binaries this build no longer produces.
      while IFS=$'\t' read -r asset name; do
        [[ "$name" == *"$suffix" || -e "$dir/$name" ]] || delete_asset "$asset"
      done < "$assets"

      # Binaries first, body second: a client that reads the new commit always
      # finds its binary.
      gh api --method PATCH "repos/$GH_REPO/releases/$id" \
        -f name="$title" -f body="$notes" -F prerelease=true -f make_latest=false > /dev/null
      # Clients read the body, not the tag, so moving the tag is cosmetic.
      gh api --method PATCH "repos/$GH_REPO/git/refs/tags/nightly" \
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
