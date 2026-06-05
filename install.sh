#!/usr/bin/env bash
# gt installer. Downloads the latest `gt` release binary for this platform and
# installs it to ~/.local/bin (override with GT_INSTALL_DIR).
#
#   curl -fsSL https://raw.githubusercontent.com/gt-core-labs/gt/main/install.sh | bash
#
# Defaults to the rolling `latest` release (always tracks main). Pin a stable build
# with GT_VERSION=vX.Y.Z. After install, keep it current with `gt update`.
set -euo pipefail

REPO="gt-core-labs/gt"
BIN="gt"

os="$(uname -s)"
arch="$(uname -m)"
case "${os}-${arch}" in
  Linux-x86_64)   target="x86_64-unknown-linux-gnu" ;;
  Darwin-arm64)   target="aarch64-apple-darwin" ;;
  *) echo "gt: unsupported platform ${os}-${arch} (build from source: cargo install --git https://github.com/${REPO})" >&2; exit 1 ;;
esac

version="${GT_VERSION:-latest}"
asset="${BIN}-${target}.tar.gz"
url="https://github.com/${REPO}/releases/download/${version}/${asset}"
dest="${GT_INSTALL_DIR:-${HOME}/.local/bin}"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT
echo "gt: downloading ${version} (${target})"
curl -fsSL "${url}" -o "${tmp}/${asset}"
tar -xzf "${tmp}/${asset}" -C "${tmp}"
mkdir -p "${dest}"
install -m 0755 "${tmp}/${BIN}" "${dest}/${BIN}"

echo "gt: installed ${version} -> ${dest}/${BIN}"
case ":${PATH}:" in
  *":${dest}:"*) ;;
  *) echo "gt: add ${dest} to your PATH (e.g. export PATH=\"${dest}:\$PATH\")" ;;
esac
echo "gt: run 'gt init' to connect, or 'gt update' to upgrade later."
