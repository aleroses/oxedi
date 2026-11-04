#!/usr/bin/env bash
# Installs actionlint, shellcheck and zizmor (prebuilt, Linux x86_64) into ~/.local/bin, and puts
# that on the PATH of the following GitHub Actions steps. Each download is checked against a
# pinned SHA-256; bumping a version means bumping its checksum here too.
set -euo pipefail

ACTIONLINT_VERSION="1.7.12"
ACTIONLINT_SHA256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
SHELLCHECK_VERSION="0.11.0"
SHELLCHECK_SHA256="8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198"
ZIZMOR_VERSION="1.30.1"
ZIZMOR_SHA256="e65324f4430c2717591937edcec90ccbefaf14c174f8ec9415e03ca875b46e1a"

bin="${LINT_TOOLS_BIN:-$HOME/.local/bin}"
mkdir -p "$bin"
if [ -n "${GITHUB_PATH:-}" ]; then echo "$bin" >> "$GITHUB_PATH"; fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"

# Downloads URL and fails, naming the URL and both hashes, unless it has the pinned SHA-256.
fetch() {
  local url=$1 expected=$2 file actual
  file=$(basename "$url")
  curl -fsSLO "$url"
  actual=$(sha256sum "$file" | cut -d' ' -f1)
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch for $url: expected $expected, got $actual" >&2
    exit 1
  fi
}

name="actionlint_${ACTIONLINT_VERSION}_linux_amd64"
fetch "https://github.com/rhysd/actionlint/releases/download/v$ACTIONLINT_VERSION/$name.tar.gz" "$ACTIONLINT_SHA256"
tar xzf "$name.tar.gz" actionlint
install actionlint "$bin/"

name="shellcheck-v$SHELLCHECK_VERSION.linux.x86_64"
fetch "https://github.com/koalaman/shellcheck/releases/download/v$SHELLCHECK_VERSION/$name.tar.xz" "$SHELLCHECK_SHA256"
tar xJf "$name.tar.xz"
install "shellcheck-v$SHELLCHECK_VERSION/shellcheck" "$bin/"

name="zizmor-x86_64-unknown-linux-gnu"
fetch "https://github.com/zizmorcore/zizmor/releases/download/v$ZIZMOR_VERSION/$name.tar.gz" "$ZIZMOR_SHA256"
tar xzf "$name.tar.gz" zizmor
install zizmor "$bin/"
