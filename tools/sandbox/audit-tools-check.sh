#!/usr/bin/env bash
# Proves the audit tools of the image do their job, not only print a version: cargo-deny fetches
# the advisory database (network on, no project code), then checks a scratch crate against it with
# the network off; pip-audit reads its own version. Run: tools/sandbox/audit-tools-check.sh IMAGE
set -euo pipefail

image=${1:?usage: audit-tools-check.sh IMAGE}
deny_toml="$(cd "$(dirname "$0")" && pwd)/deny.toml"
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
mkdir -p "$work/crate/src" "$work/advisory"
chmod 777 "$work/advisory"
echo 'fn main() {}' > "$work/crate/src/main.rs"
printf '[package]\nname = "scratch"\nversion = "0.1.0"\nedition = "2021"\n' > "$work/crate/Cargo.toml"
printf 'version = 3\n\n[[package]]\nname = "scratch"\nversion = "0.1.0"\n' > "$work/crate/Cargo.lock"
chmod -R a+rwX "$work/crate"

run=(docker run --rm --pull never --read-only --cap-drop ALL --security-opt no-new-privileges
  --user "$(id -u):$(id -g)" --tmpfs /tmp --env CARGO_HOME=/tmp/cargo
  --volume "$work/crate:/crate" --volume "$deny_toml:/sandbox/deny.toml:ro"
  --volume "$work/advisory:/sandbox/advisory" --workdir /crate)

"${run[@]}" "$image" cargo-deny --manifest-path Cargo.toml fetch --config /sandbox/deny.toml db
"${run[@]}" --network none "$image" cargo-deny --manifest-path Cargo.toml check \
  --config /sandbox/deny.toml --disable-fetch advisories sources bans
"${run[@]}" --network none "$image" pip-audit --version
echo "audit-tools-check: ok"
