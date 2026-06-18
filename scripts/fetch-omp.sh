#!/usr/bin/env bash
# Download omp (oh-my-pi / pi-coding-agent) binaries for all supported Tauri
# targets and place them into src-tauri/binaries/ with the exact naming Tauri's
# sidecar loader expects: `omp-<rust-target-triple>[.exe]`.
#
# This vendors omp purely for OPTIONAL Tauri sidecar bundling — so a desktop
# build can ship a self-contained engine binary instead of relying on a global
# install. It is NOT how omp lands on a no-admin HPC compute node: there the
# canonical install is `curl -fsSL https://omp.sh/install | sh`, which drops a
# self-contained ~150MB binary into ~/.local/bin with no root and no Bun/Node
# at runtime. (Note: the linux HPC remote path has not yet been validated on a
# real cluster — glibc compatibility of the prebuilt linux binary is unconfirmed.)
#
# Upstream release assets (can1357/oh-my-pi) are named:
#   omp-darwin-arm64  omp-darwin-x64  omp-linux-x64  omp-linux-arm64  omp-windows-x64.exe
#
# We also keep the linux-x64 build under a -musl alias (not wired into
# externalBin) — the portable binary we scp to remote servers when the user
# asks Operon to install omp for them.
#
# Run once. Idempotent. Re-run (or set OMP_VERSION) to upgrade the pinned version.

set -euo pipefail

REPO="can1357/oh-my-pi"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/src-tauri/binaries"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$OUT_DIR"

# Resolve the release tag to fetch. Override with OMP_VERSION (e.g. v16.0.5).
# Discovery order: explicit env var -> gh CLI -> GitHub API -> hard fail.
resolve_version() {
  if [[ -n "${OMP_VERSION:-}" ]]; then
    echo "$OMP_VERSION"
    return 0
  fi

  if command -v gh >/dev/null 2>&1; then
    local tag
    if tag="$(gh release view --repo "$REPO" --json tagName --jq .tagName 2>/dev/null)" && [[ -n "$tag" ]]; then
      echo "$tag"
      return 0
    fi
  fi

  # GitHub API fallback (no auth needed for public releases).
  local api="https://api.github.com/repos/${REPO}/releases/latest"
  local tag
  tag="$(curl -fsSL "$api" 2>/dev/null | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')" || true
  if [[ -n "$tag" ]]; then
    echo "$tag"
    return 0
  fi

  echo "ERROR: could not discover latest release for $REPO." >&2
  echo "       Set OMP_VERSION=<tag> (e.g. OMP_VERSION=v16.0.5) and re-run." >&2
  return 1
}

# Download one raw release asset and place it under the Tauri target naming.
download_asset() {
  local asset_name="$1"     # e.g. omp-darwin-arm64
  local target_triple="$2"  # Tauri rust target triple for sidecar naming
  local ext="$3"            # "" or ".exe"
  local url="https://github.com/${REPO}/releases/download/${VERSION}/${asset_name}"
  local out_name="omp-${target_triple}${ext}"

  echo "  Downloading $asset_name"
  curl -fsSL "$url" -o "$TMP_DIR/$asset_name"
  cp "$TMP_DIR/$asset_name" "$OUT_DIR/$out_name"
  chmod +x "$OUT_DIR/$out_name" || true
  echo "  wrote  $OUT_DIR/$out_name"
}

VERSION="$(resolve_version)"

echo "Fetching omp ${VERSION} from ${REPO} into ${OUT_DIR}"

download_asset "omp-darwin-arm64"      "aarch64-apple-darwin"      ""
download_asset "omp-darwin-x64"        "x86_64-apple-darwin"       ""
download_asset "omp-linux-x64"         "x86_64-unknown-linux-gnu"  ""
download_asset "omp-linux-arm64"       "aarch64-unknown-linux-gnu" ""
download_asset "omp-windows-x64.exe"   "x86_64-pc-windows-msvc"    ".exe"

# Keep a copy of the linux-x64 build under the -musl name for remote-install
# deployment (mirrors the ripgrep vendoring convention).
cp "$OUT_DIR/omp-x86_64-unknown-linux-gnu" "$OUT_DIR/omp-x86_64-unknown-linux-musl"
chmod +x "$OUT_DIR/omp-x86_64-unknown-linux-musl" || true

echo
echo "Done. Vendored binaries:"
ls -lh "$OUT_DIR"/omp-*
