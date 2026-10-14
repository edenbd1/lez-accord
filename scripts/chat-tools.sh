#!/usr/bin/env bash
# Fetch what scripts/e2e-chat.sh runs the room on: the released, self-contained
# `logosctl` (logos-co/logos-logoscore-cli, the headless Logos Core runtime).
# Nothing is built. Each member's session then installs chat_module (and its
# delivery_module) from the official Logos module catalog, the same packages
# Basecamp installs.
#
#   scripts/chat-tools.sh /tmp/accord-chat     # then CHAT_TOOLS=/tmp/accord-chat
set -euo pipefail
dir="${1:?usage: chat-tools.sh <dir>}"
release="${LOGOSCTL_RELEASE:-0.3.2}"
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset=logosctl-aarch64-macos ;;
  Linux-x86_64) asset=logosctl-x86_64-linux ;;
  Linux-aarch64) asset=logosctl-aarch64-linux ;;
  *) echo "unsupported platform" >&2; exit 1 ;;
esac
mkdir -p "$dir" && cd "$dir"
gh release download "$release" -R logos-co/logos-logoscore-cli -p "$asset.tar.gz" --clobber
rm -rf logosctl && mkdir logosctl && tar xzf "$asset.tar.gz" -C logosctl
[ "$(uname -s)" = Darwin ] && xattr -dr com.apple.quarantine logosctl 2>/dev/null || true
# a wrapper, not a symlink: logosctl finds its bundled modules next to its real path
printf '#!/bin/sh\nexec "%s" "$@"\n' "$PWD/logosctl/$asset/bin/logosctl" > logosctl-bin
chmod +x logosctl-bin
./logosctl-bin --version
echo "CHAT_TOOLS=$dir"
