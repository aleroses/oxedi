#!/usr/bin/env bash
# Checks that deploy-docs.sh hands its push token to `git push` alone, against a local bare
# repository standing in for origin. mike, mkdocs and git are stubs on PATH: the mike stub
# commits its version onto the local gh-pages as mike does, the git stub logs what each git
# command saw and runs the real git.
# Run: .github/scripts/tests/deploy-docs-token.sh
set -uo pipefail

scripts=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
REAL_GIT=$(command -v git)
TOKEN=ghs_deploy_docs_test_token
AUTH=$(printf 'x-access-token:%s' "$TOKEN" | base64 | tr -d '\n')
# The stubs read the expected header and the two secrets (the token and its base64) from files:
# exported, they would sit in the environment of every process the probe below checks.
export REAL_GIT EXPECTED="$tmp/expected" SECRETS="$tmp/secrets" PROBE="$tmp/probe" TEST_PID=$$
printf 'AUTHORIZATION: basic %s\n' "$AUTH" > "$EXPECTED"
printf '%s\n%s\n' "$TOKEN" "$AUTH" > "$SECRETS"
export RETRY_DELAY_UNIT=0 GITHUB_SHA=0123456789abcdef
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
failed=0

expect() {
  local what=$1 expected=$2 actual=$3
  if [ "$expected" != "$actual" ]; then
    echo "FAIL: $what: expected $expected, got $actual"
    failed=1
  fi
}

stubs="$tmp/bin"
mkdir -p "$stubs"
# git: logs `push <whether the header is exactly the expected one>` for a push and
# `<subcommand> <GIT_CONFIG_COUNT>` for any other command, then runs the real git.
cat > "$stubs/git" <<'S'
#!/usr/bin/env bash
args=("$@")
sub=""
while [ $# -gt 0 ]; do
  case $1 in
    -C | -c) shift 2 ;;
    -*) shift ;;
    *) sub=$1; break ;;
  esac
done
if [ "$sub" = push ]; then
  if [ "${GIT_CONFIG_VALUE_0-}" = "$(cat "$EXPECTED")" ] && [ "${GIT_CONFIG_COUNT-}" = 1 ]; then
    echo "push yes" >> "$GIT_LOG"
  else
    echo "push no" >> "$GIT_LOG"
  fi
else
  echo "$sub ${GIT_CONFIG_COUNT-unset}" >> "$GIT_LOG"
fi
exec "$REAL_GIT" "${args[@]}"
S
# The probe prints, for the process that runs it: the token variable, GIT_CONFIG_COUNT, how many
# extraheader entries git config holds, how many of its own and its ancestors' environments (up
# to this test script) hold a secret, whether the walk reached this script, and whether file
# descriptor 3 is open. A clean process prints `unset unset 0 0 yes closed`.
cat > "$PROBE" <<'S'
#!/usr/bin/env bash
holding=0
reached=no
pid=$$
while [ "$pid" -gt 1 ]; do
  if grep -qaF -f "$SECRETS" "/proc/$pid/environ"; then holding=$((holding + 1)); fi
  if [ "$pid" = "$TEST_PID" ]; then reached=yes; break; fi
  pid=$(awk '/^PPid:/ { print $2 }' "/proc/$pid/status")
done
fd3=closed
if [ -e /proc/$$/fd/3 ]; then fd3=open; fi
echo "${PAGES_PUSH_TOKEN-unset} ${GIT_CONFIG_COUNT-unset} $(git config --get-regexp extraheader | wc -l) $holding $reached $fd3"
S
# mkdocs and mike: log what the probe prints. `mike deploy ... VERSION` commits VERSION/index.html
# and versions.json onto the local gh-pages. When the file $RACE exists, the first deploy removes
# it and first pushes a commit to origin's gh-pages from another clone, so the deploy's own push
# is rejected.
cat > "$stubs/mkdocs" <<'S'
#!/usr/bin/env bash
echo "mkdocs $("$PROBE")" >> "$TOOL_LOG"
S
cat > "$stubs/mike" <<'S'
#!/usr/bin/env bash
set -eu
echo "mike $("$PROBE")" >> "$TOOL_LOG"
mkdocs build
[ "$1" = deploy ] || exit 0
version=${!#}
if [ -f "$RACE" ]; then
  rm "$RACE"
  "$REAL_GIT" -C "$OTHER" fetch -q origin gh-pages
  "$REAL_GIT" -C "$OTHER" reset -q --hard FETCH_HEAD
  "$REAL_GIT" -C "$OTHER" commit -q --allow-empty -m "other deploy"
  "$REAL_GIT" -C "$OTHER" push -q origin HEAD:gh-pages
fi
GIT_INDEX_FILE=$(mktemp -u)
export GIT_INDEX_FILE
git read-tree gh-pages
page=$(echo "<html>$version</html>" | git hash-object -w --stdin)
versions=$(printf '[{"version":"%s","title":"%s","aliases":[]}]' "$version" "$version" | git hash-object -w --stdin)
git update-index --add --cacheinfo "100644,$page,$version/index.html"
git update-index --add --cacheinfo "100644,$versions,versions.json"
commit=$(git commit-tree "$(git write-tree)" -p gh-pages -m "Deployed $version")
git update-ref refs/heads/gh-pages "$commit"
rm -f "$GIT_INDEX_FILE"
S
chmod +x "$stubs"/* "$PROBE"

# A fresh origin with a gh-pages branch, a clone of it holding the deploy scripts and the
# viewer, and another clone that can push to origin. Sets dir and exports the logs.
setup() {
  dir="$tmp/$1"
  mkdir -p "$dir"
  export GIT_LOG="$dir/git.log" TOOL_LOG="$dir/tools.log" RACE="$dir/race" OTHER="$dir/other"
  : > "$GIT_LOG"
  : > "$TOOL_LOG"
  "$REAL_GIT" init -q --bare "$dir/origin.git"
  "$REAL_GIT" clone -q "$dir/origin.git" "$OTHER" 2> /dev/null
  "$REAL_GIT" -C "$OTHER" checkout -q -b gh-pages
  "$REAL_GIT" -C "$OTHER" commit -q --allow-empty -m "gh-pages start"
  "$REAL_GIT" -C "$OTHER" push -q origin gh-pages
  "$REAL_GIT" init -q -b main "$dir/work"
  mkdir -p "$dir/work/.github" "$dir/work/viewer/pkg" "$dir/work/docs"
  cp -R "$scripts" "$dir/work/.github/scripts"
  echo "<meta http-equiv=\"Content-Security-Policy\" content=\"connect-src 'none'\">" > "$dir/work/viewer/index.html"
  echo "export {};" > "$dir/work/viewer/pkg/index.js"
  echo "# llms" > "$dir/work/docs/llms.txt"
  "$REAL_GIT" -C "$dir/work" add -A
  "$REAL_GIT" -C "$dir/work" commit -q -m work
  "$REAL_GIT" -C "$dir/work" remote add origin "$dir/origin.git"
}
origin_pages() { "$REAL_GIT" -C "$dir/origin.git" rev-parse gh-pages; }
deploy() { (cd "$dir/work" && PATH="$stubs:$PATH" "$@" > "$dir/out" 2>&1); }
differs() { if [ "$1" != "$2" ]; then echo yes; else echo no; fi; }
# grep -c with its options and pattern, then the file.
count() { grep -c "$@"; }

# A push run: mike and mkdocs see no token, every push carries the header, gh-pages moves.
setup push
before=$(origin_pages)
deploy .github/scripts/deploy-docs.sh dev 3<<< "$TOKEN"
expect "a push run exits 0" 0 $?
expect "mike_sees_no_token: mike and mkdocs ran" 2 "$(wc -l < "$TOOL_LOG")"
expect "mike_sees_no_token: no tool or process above it held the token, GIT_CONFIG_COUNT or an extraheader, and fd 3 was closed" 2 "$(count -x '[a-z]* unset unset 0 0 yes closed' "$TOOL_LOG")"
expect "push_carries_the_header: the site and the viewer were pushed" 2 "$(count -x 'push yes' "$GIT_LOG")"
expect "push_carries_the_header: no push went without the header" 0 "$(count -x 'push no' "$GIT_LOG")"
expect "push_carries_the_header: no other git command saw GIT_CONFIG_COUNT" 0 "$(grep -v '^push ' "$GIT_LOG" | grep -cv ' unset$')"
expect "push_carries_the_header: origin's gh-pages moved" yes "$(differs "$before" "$(origin_pages)")"
"$REAL_GIT" -C "$dir/origin.git" cat-file -e gh-pages:viewer/dev/index.html
expect "push_carries_the_header: origin's gh-pages holds the viewer" 0 $?
"$REAL_GIT" -C "$dir/origin.git" cat-file -e gh-pages:dev/index.html
expect "push_carries_the_header: origin's gh-pages holds the site" 0 $?
expect "token_never_printed: the output holds no token" 0 "$(grep -cF -e "$TOKEN" -e "$AUTH" "$dir/out")"
expect "token_never_printed: .git/config holds no token" 0 "$(grep -cF -e "$TOKEN" -e "$AUTH" "$dir/work/.git/config")"

# The first push is rejected because another deploy moved gh-pages; the retry pushes again with
# the header and succeeds on top of it.
setup retry
touch "$RACE"
deploy .github/scripts/deploy-docs.sh dev 3<<< "$TOKEN"
expect "retry_pushes_again: the run exits 0" 0 $?
grep -q "the push was rejected because gh-pages moved" "$dir/out"
expect "retry_pushes_again: the first push was rejected" 0 $?
expect "retry_pushes_again: three pushes (rejected, retried, viewer), all with the header" 3 "$(count -x 'push yes' "$GIT_LOG")"
expect "retry_pushes_again: no push went without the header" 0 "$(count -x 'push no' "$GIT_LOG")"
expect "retry_pushes_again: the tools never saw the token" "$(wc -l < "$TOOL_LOG")" "$(count -x '[a-z]* unset unset 0 0 yes closed' "$TOOL_LOG")"
"$REAL_GIT" -C "$dir/origin.git" merge-base --is-ancestor "$("$REAL_GIT" -C "$OTHER" rev-parse HEAD)" gh-pages
expect "retry_pushes_again: the deploy landed on top of the other one" 0 $?
"$REAL_GIT" -C "$dir/origin.git" cat-file -e gh-pages:dev/index.html
expect "retry_pushes_again: origin's gh-pages holds the site" 0 $?
expect "token_never_printed: the retry's output holds no token" 0 "$(grep -cF -e "$TOKEN" -e "$AUTH" "$dir/out")"

# A client hook planted in the checkout by an earlier build step never runs on the push, so it
# cannot read the header from the push's environment.
setup hooks
mkdir -p "$dir/work/.git/hooks"
cat > "$dir/work/.git/hooks/pre-push" <<S
#!/usr/bin/env bash
echo "\${GIT_CONFIG_VALUE_0-}" >> "$dir/hook.log"
S
chmod +x "$dir/work/.git/hooks/pre-push"
deploy .github/scripts/deploy-docs.sh dev 3<<< "$TOKEN"
expect "push_runs_no_hook: the run exits 0" 0 $?
expect "push_runs_no_hook: the planted pre-push hook never ran" no "$(if [ -e "$dir/hook.log" ]; then echo yes; else echo no; fi)"
expect "push_runs_no_hook: both pushes still carried the header" 2 "$(count -x 'push yes' "$GIT_LOG")"

# Under `bash -x` the token is still never printed.
setup xtrace
deploy bash -x .github/scripts/deploy-docs.sh dev 3<<< "$TOKEN"
expect "token_never_printed: a traced run exits 0" 0 $?
expect "token_never_printed: a traced run prints no token" 0 "$(grep -cF -e "$TOKEN" -e "$AUTH" "$dir/out")"

# A push run with file descriptor 3 closed, or empty, refuses before mike runs.
missing="deploy-docs: a push run reads its token from file descriptor 3, which is empty or closed (the docs.yml deploy step writes the workflow's GITHUB_TOKEN there); --no-push builds without it"
setup missing
before=$(origin_pages)
deploy .github/scripts/deploy-docs.sh dev 3<&-
expect "missing_token_refuses: closed fd 3 exits 1" 1 $?
expect "missing_token_refuses: closed fd 3, the message" "$missing" "$(cat "$dir/out")"
deploy .github/scripts/deploy-docs.sh dev 3< /dev/null
expect "missing_token_refuses: empty fd 3 exits 1" 1 $?
expect "missing_token_refuses: empty fd 3, the message" "$missing" "$(cat "$dir/out")"
expect "missing_token_refuses: mike never ran" 0 "$(wc -l < "$TOOL_LOG")"
expect "missing_token_refuses: origin's gh-pages did not move" "$before" "$(origin_pages)"

# A push run refuses when the token is still in its environment, even with a good fd 3.
setup envtoken
before=$(origin_pages)
PAGES_PUSH_TOKEN=$TOKEN deploy .github/scripts/deploy-docs.sh dev 3<<< "$TOKEN"
expect "env_token_refuses: exits 1" 1 $?
expect "env_token_refuses: the message" "deploy-docs: PAGES_PUSH_TOKEN is set in the environment, where mike would inherit it; the docs.yml deploy step starts this script as \`env -u PAGES_PUSH_TOKEN\` and writes the token to file descriptor 3" "$(cat "$dir/out")"
expect "env_token_refuses: mike never ran" 0 "$(wc -l < "$TOOL_LOG")"
expect "env_token_refuses: origin's gh-pages did not move" "$before" "$(origin_pages)"

# A descriptor 3 that stays open and silent is refused after the timeout instead of hanging.
setup fdtimeout
before=$(origin_pages)
deploy .github/scripts/deploy-docs.sh dev 3< <(sleep 9)
expect "fd_timeout_refuses: exits 1" 1 $?
expect "fd_timeout_refuses: the message" "deploy-docs: nothing arrived on file descriptor 3 within 5 seconds (the docs.yml deploy step writes the workflow's GITHUB_TOKEN there and closes it); --no-push builds without it" "$(cat "$dir/out")"
expect "fd_timeout_refuses: mike never ran" 0 "$(wc -l < "$TOOL_LOG")"
expect "fd_timeout_refuses: origin's gh-pages did not move" "$before" "$(origin_pages)"

# When the header cannot be built, nothing is pushed and the run says why.
setup nobase64
mkdir -p "$dir/bin"
printf '#!/usr/bin/env bash\nexit 1\n' > "$dir/bin/base64"
chmod +x "$dir/bin/base64"
before=$(origin_pages)
PATH="$dir/bin:$PATH" deploy .github/scripts/deploy-docs.sh dev 3<<< "$TOKEN"
expect "a failed base64 fails the run" 1 $?
expect "a failed base64 is named" 1 "$(count -x "deploy-docs: could not encode the push token for the Authorization header (base64 failed); nothing was pushed" "$dir/out")"
expect "a failed base64 pushes nothing" 0 "$(count '^push ' "$GIT_LOG")"
expect "a failed base64 leaves origin's gh-pages alone" "$before" "$(origin_pages)"

# --no-push needs no token, pushes nothing and moves only the local gh-pages.
setup nopush
before=$(origin_pages)
"$REAL_GIT" -C "$dir/work" fetch -q origin gh-pages:gh-pages
deploy .github/scripts/deploy-docs.sh --no-push dev
expect "a --no-push run exits 0 without a token" 0 $?
expect "a --no-push run pushes nothing" 0 "$(count '^push ' "$GIT_LOG")"
expect "a --no-push run leaves origin's gh-pages alone" "$before" "$(origin_pages)"
"$REAL_GIT" -C "$dir/work" cat-file -e gh-pages:viewer/dev/index.html
expect "a --no-push run commits the viewer onto the local gh-pages" 0 $?

if [ "$failed" -eq 0 ]; then echo "deploy-docs-token: ok"; else cat "$dir/out"; fi
exit "$failed"
