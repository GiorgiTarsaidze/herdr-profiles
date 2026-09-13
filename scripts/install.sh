#!/bin/sh
# Runs once at `herdr plugin install` time: puts the herdr-profiles binary in
# bin/ (prebuilt release when available, otherwise `cargo build --release`),
# then binds the chooser to a free key in config.toml.
set -eu

REPO="GiorgiTarsaidze/herdr-profiles"
NAME="herdr-profiles"
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' herdr-plugin.toml | head -1)
mkdir -p bin

have() { command -v "$1" >/dev/null 2>&1; }
say() { echo "$NAME: $*" >&2; }

find_cargo() {
  have cargo && return 0
  for dir in "$HOME/.cargo/bin" /usr/local/cargo/bin /opt/homebrew/bin /usr/local/bin; do
    if [ -x "$dir/cargo" ]; then
      PATH="$dir:$PATH"; export PATH
      return 0
    fi
  done
  return 1
}

build_from_source() {
  find_cargo || return 1
  say "building from source with $(cargo --version)"
  cargo build --release --quiet || return 1
  install -m 0755 "target/release/$NAME" "bin/$NAME"
}

download_release() {
  os=$(uname -s); arch=$(uname -m)
  case "$os" in Darwin) os=darwin ;; Linux) os=linux ;; *) return 1 ;; esac
  case "$arch" in x86_64|amd64) arch=amd64 ;; arm64|aarch64) arch=arm64 ;; *) return 1 ;; esac
  asset="$NAME-$os-$arch"
  base="https://github.com/$REPO/releases/download/v$VERSION"
  tmp=$(mktemp -d) || return 1
  trap 'rm -rf "$tmp"' EXIT
  if have curl; then
    dl() { curl -fsSL --retry 2 -o "$2" "$1"; }
  elif have wget; then
    dl() { wget -q -O "$2" "$1"; }
  else
    return 1
  fi
  say "fetching $base/$asset"
  dl "$base/$asset" "$tmp/$asset" || return 1
  dl "$base/SHA256SUMS" "$tmp/SHA256SUMS" || return 1
  expected=$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
  [ -n "$expected" ] || return 1
  if have sha256sum; then actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
  elif have shasum; then actual=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
  else return 1; fi
  if [ "$expected" != "$actual" ]; then
    say "checksum mismatch for $asset, refusing to install it"
    return 1
  fi
  install -m 0755 "$tmp/$asset" "bin/$NAME"
  say "installed prebuilt $asset v$VERSION"
}

if [ "${HERDR_PROFILES_BUILD:-}" = "source" ]; then
  build_from_source || { say "cargo build failed"; exit 1; }
elif download_release; then
  :
elif build_from_source; then
  :
elif [ -x "bin/$NAME" ]; then
  say "no release download and no Rust toolchain; keeping the existing bin/$NAME"
else
  say "no prebuilt binary for this platform and no Rust toolchain found."
  say "install Rust from https://rustup.rs and run: herdr plugin install $REPO"
  exit 1
fi

"./bin/$NAME" setup || say "keybinding setup skipped; run: ./bin/$NAME setup --key prefix+a"
