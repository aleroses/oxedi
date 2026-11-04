#!/usr/bin/env bash
# Proves that sandbox-pr keeps a hostile change away from the host, the network and the shared
# caches: it plants the probes of tests/malicious/ (a build script and a test hook) in a temporary
# clone of the checkout, commits them on a local branch there (never pushed, never in the host
# repository), reviews that branch with sandbox_pr.py and checks what the probes recorded. One line
# per check, "ok" when it holds, "FAIL" when it does not; it exits 1 when a line failed.
# Run it through `make sandbox-pr-check`, which passes the image and the checkout.
set -uo pipefail

image=${1:?usage: malicious-pr.sh IMAGE CHECKOUT (make sandbox-pr-check passes them)}
checkout=$(cd "${2:?usage: malicious-pr.sh IMAGE CHECKOUT}" && pwd)
sandbox_dir="$checkout/tools/sandbox"
probes="$sandbox_dir/tests/malicious"
branch=malicious-probe
target='cargo build -p oxedi_core && make py-test'
failures=0

# Every git here runs without the host's configuration, hooks, attributes file or file transport;
# `local` allows the local transport for the one clone of the checkout.
git_isolated() {
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GIT_LFS_SKIP_SMUDGE=1 LC_ALL=C \
    git -c core.hooksPath=/dev/null -c core.attributesFile=/dev/null -c protocol.file.allow=never "$@"
}
git_local() { git_isolated -c protocol.file.allow=always "$@"; }

# check WHAT COMMAND...: prints "ok   WHAT" when COMMAND succeeds, "FAIL WHAT" otherwise.
check() {
  local what=$1
  shift
  if "$@" > /dev/null 2>&1; then
    echo "ok   $what"
  else
    echo "FAIL $what"
    failures=$((failures + 1))
  fi
}

# repo_state: the host repository's refs and object counts, to compare before and after.
repo_state() {
  git_isolated -C "$checkout" for-each-ref --format='%(refname) %(objectname)'
  git_isolated -C "$checkout" count-objects -v | grep -E '^(count|in-pack|packs):'
}

# planted FILE: the probe with the host home and the canary written in. It does no checking: a path
# holding a quote or a backslash would break the literals, so the loop before the first probe is
# planted refuses one.
planted() {
  local text
  text=$(< "$1")
  text=${text//@HOST_HOME@/"$HOME"}
  printf '%s\n' "${text//@HOST_CANARY@/"$canary"}"
}

# all_refused FILE COUNT: the file holds COUNT result lines, each "denied" or "failed".
all_refused() {
  [ "$(grep -c . "$1")" -eq "$2" ] && ! grep -vE ': (denied|failed)$' "$1"
}

# flagged_as PATH KIND: report.json lists PATH among the touched paths with KIND.
flagged_as() {
  python3 -I -c 'import json, sys
report = json.load(open(sys.argv[1]))
sys.exit(not any(t["path"] == sys.argv[2] and sys.argv[3] in t["kinds"] for t in report["touched"]))' \
    "$review/report.json" "$1" "$2"
}

# no_home_result: no result file of the probes landed in the host home outside the sandbox storage.
no_home_result() {
  [ -z "$(find "$HOME" -maxdepth 3 -name 'malicious-*.txt' -not -path "$cache/*" 2> /dev/null)" ]
}

for probe in build.rs conftest_probe.py; do
  [ -f "$probes/$probe" ] || { echo "malicious-pr: $probes/$probe is missing" >&2; exit 2; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/canary"
canary="$work/canary/canary.txt"
echo "sandbox-pr-check canary $$" > "$canary"
canary_sum=$(sha256sum < "$canary")
for path in "$HOME" "$canary"; do
  case $path in *[\"\\]*) echo "malicious-pr: $path holds a quote or a backslash" >&2; exit 2 ;; esac
done
repo_before=$(repo_state)

clone="$work/clone"
git_local clone --quiet --no-hardlinks --no-recurse-submodules -- "$checkout" "$clone" || exit 2
git_isolated -C "$clone" switch --quiet --create "$branch" || exit 2
planted "$probes/build.rs" > "$clone/crates/oxedi_core/build.rs"
planted "$probes/conftest_probe.py" | { echo; cat; } >> "$clone/crates/oxedi_py/tests/conftest.py"
git_isolated -C "$clone" add crates/oxedi_core/build.rs crates/oxedi_py/tests/conftest.py || exit 2
git_isolated -C "$clone" -c user.name=sandbox-pr-check -c user.email=sandbox-pr-check@invalid \
  commit --quiet --no-verify --message "a synthetic hostile change" || exit 2
tested=$(git_isolated -C "$clone" rev-parse HEAD) || exit 2

# The storage paths come from the sandbox's own modules, so the check cannot drift from them:
# the cache root, the review directory of the planted commit and the target/ of its clone.
mapfile -t paths < <(python3 -I -c 'import os, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from layout import path_hash
from storage import cache_root, reviews
cache = cache_root(dict(os.environ))
reviews(cache).mkdir(mode=0o700, parents=True, exist_ok=True)
review = reviews(cache).resolve() / f"ref-{sys.argv[2]}-{sys.argv[3][:12]}"
print(cache, review, cache / path_hash(review / "checkout") / "target", sep="\n")' "$sandbox_dir" "$branch" "$tested") || exit 2
[ "${#paths[@]}" -eq 3 ] || { echo "malicious-pr: cannot compute the storage paths" >&2; exit 2; }
cache=${paths[0]}
review=${paths[1]}
results=${paths[2]}

env -u SANDBOX_PR -u SANDBOX_HEAD SANDBOX_REF="$branch" SANDBOX_SOURCE="$clone" SANDBOX_CONFIRM=yes \
  SANDBOX_TARGET="$target" python3 -I "$sandbox_dir/sandbox_pr.py" --image "$image" --kind base --checkout "$checkout"
status=$?

echo "malicious-pr: sandbox-pr exited $status; the checks:"
check "sandbox-pr ran the target to the end: exit 0" test "$status" -eq 0
check "the report flags crates/oxedi_core/build.rs as BUILD_SCRIPT" flagged_as crates/oxedi_core/build.rs BUILD_SCRIPT
check "the report flags crates/oxedi_py/tests/conftest.py as TEST_HOOK" \
  flagged_as crates/oxedi_py/tests/conftest.py TEST_HOOK
check "the build script ran: target/malicious-build.txt exists" test -f "$results/malicious-build.txt"
check "the test hook ran: target/malicious-conftest.txt exists" test -f "$results/malicious-conftest.txt"
check "every attempt of the build script was denied or failed" all_refused "$results/malicious-build.txt" 5
check "every attempt of the test hook was denied or failed" all_refused "$results/malicious-conftest.txt" 5
check "the host canary is unchanged" test "$(sha256sum < "$canary")" = "$canary_sum"
check "the canary's directory holds only the canary" test "$(ls -A "$work/canary")" = canary.txt
check "the shared cargo cache holds no x" test ! -e "$cache/cargo/x"
check "the host home holds no result file of the probes" no_home_result
check "the host repository has the same refs and object counts" test "$(repo_state)" = "$repo_before"
for file in malicious-build.txt malicious-conftest.txt; do
  [ -f "$results/$file" ] && sed "s/^/  $file: /" "$results/$file"
done

if [ "$failures" -ne 0 ]; then
  echo "malicious-pr: failed checks: $failures; the review is kept in $review"
  exit 1
fi
# The review holds the planted change and its storage: nothing of it outlives a passing check.
python3 -I -c 'import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from storage import forget
forget(Path(sys.argv[2]), Path(sys.argv[3]))' "$sandbox_dir" "$cache" "$review"
