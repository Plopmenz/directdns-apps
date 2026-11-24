#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION_FILE=android-version.txt
KEY_DIR=$(realpath "${DIRECTDNS_KEY_DIR:-$PWD/keys}")

for file in upload.jks key-alias.txt upload-password.txt; do
  if [[ ! -f "$KEY_DIR/$file" ]]; then
    echo "Missing $KEY_DIR/$file" >&2
    exit 1
  fi
done

current=$(sed -n 's/^versionCode=//p' "$VERSION_FILE")
if [[ ! "$current" =~ ^[0-9]+$ ]]; then
  echo "Invalid versionCode in $VERSION_FILE: $current" >&2
  exit 1
fi

next=$((current + 1))
tmp=$(mktemp "$VERSION_FILE.tmp.XXXXXX")
awk -v code="$next" '
  /^versionCode=/ { print "versionCode=" code; next }
  { print }
' "$VERSION_FILE" >"$tmp"
mv "$tmp" "$VERSION_FILE"

# Stage the new value so the Git-based flake source sees it during the build.
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  git add -- "$VERSION_FILE"
fi

echo "versionCode=$next"
echo "versionName=$(sed -n 's/^versionName=//p' "$VERSION_FILE")"

DIRECTDNS_KEY_DIR="$KEY_DIR" nix build --impure .#android-aab "$@"
