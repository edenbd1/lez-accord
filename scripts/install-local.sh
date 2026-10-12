#!/usr/bin/env bash
# Install .lgx packages into a Basecamp user directory, the way Basecamp's own
# package manager lays them out: core modules under modules/<name>/, apps
# under plugins/<name>/, one platform variant flattened, plus manifest.json, the
# package's top-level assets (the icon) and a `variant` file naming it.
#
#   scripts/install-local.sh <user-dir> <package.lgx>...
#
# Then start Basecamp on that directory, which keeps it apart from your own
# Basecamp instance:
#   LogosBasecamp --user-dir <user-dir>
set -euo pipefail
dir=${1:?usage: install-local.sh <user-dir> <package.lgx>...}; shift
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) variant=darwin-arm64 ;;
  Linux-x86_64) variant=linux-amd64 ;;
  Linux-aarch64) variant=linux-arm64 ;;
  *) echo "unsupported platform" >&2; exit 1 ;;
esac
for lgx in "$@"; do
  tmp=$(mktemp -d)
  tar xzf "$lgx" -C "$tmp"
  name=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["name"])' "$tmp/manifest.json")
  type=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["type"])' "$tmp/manifest.json")
  [ -d "$tmp/variants/$variant" ] || { echo "$lgx: no $variant variant" >&2; exit 1; }
  if [ "$type" = core ]; then dest="$dir/modules/$name"; else dest="$dir/plugins/$name"; fi
  rm -rf "$dest"; mkdir -p "$dest"
  cp -R "$tmp/variants/$variant/." "$dest/"
  [ -d "$tmp/assets" ] && cp -R "$tmp/assets" "$dest/"
  cp "$tmp/manifest.json" "$dest/manifest.json"
  printf '%s' "$variant" > "$dest/variant"
  chmod -R u+w "$dest"
  rm -rf "$tmp"
  echo "installed $name ($type) -> $dest"
done
