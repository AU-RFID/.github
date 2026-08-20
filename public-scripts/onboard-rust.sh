#!/usr/bin/env bash
#
# RFID Lab onboarding TUI (PoC 2 bootstrap)
#
# Downloads the prebuilt Rust/ratatui onboarding binary for this platform from
# the latest `onboarding-tui-v*` GitHub Release and runs it. No dependencies
# beyond curl. Re-runs use the cached copy; pass --refresh to force a
# re-download.
#
#   bash <(curl -fsSL https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard-rust.sh)
#
set -euo pipefail

REPO="AU-RFID/.github"   # <org>/<repo> hosting the releases
BIN_DIR="${HOME}/.cache/rfid-onboard/bin"
BIN="${BIN_DIR}/onboarding-tui"

# --- args: --refresh is ours; everything else (e.g. --dry-run) is forwarded -
refresh=0
pass_args=()
for arg in "$@"; do
  case "$arg" in
    --refresh) refresh=1 ;;
    *) pass_args+=("$arg") ;;
  esac
done

# --- platform -> release target triple -------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "${os}-${arch}" in
  Darwin-arm64)           target="aarch64-apple-darwin" ;;
  Darwin-x86_64)          target="x86_64-apple-darwin" ;;
  Linux-x86_64)           target="x86_64-unknown-linux-musl" ;;
  Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-musl" ;;
  *)
    echo "Unsupported platform: ${os} ${arch}" >&2
    exit 1
    ;;
esac

# --- download (cached) ------------------------------------------------------
if [ "$refresh" = 1 ] || [ ! -x "$BIN" ]; then
  mkdir -p "$BIN_DIR"
  # Resolve the latest onboarding-tui-v* tag via the GitHub API (no auth needed
  # for public repos).
  tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases" \
    | grep -o '"tag_name": *"onboarding-tui-v[^"]*"' \
    | head -1 | cut -d'"' -f4)"
  if [ -z "$tag" ]; then
    echo "No onboarding-tui release found in ${REPO}." >&2
    exit 1
  fi
  url="https://github.com/${REPO}/releases/download/${tag}/onboarding-tui-${target}"
  echo "Downloading onboarding TUI ${tag} (${target})..."
  curl -fsSL "$url" -o "${BIN}.tmp"
  chmod +x "${BIN}.tmp"
  mv "${BIN}.tmp" "$BIN"
fi

exec "$BIN" ${pass_args[@]+"${pass_args[@]}"}
