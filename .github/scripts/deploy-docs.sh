#!/usr/bin/env bash
# Publishes the site to gh-pages with mike and the browser viewer next to it, then checks what
# the run was meant to publish and fails naming what is missing.
#
#   dev        deploys the `dev` version and publishes the viewer into `viewer/dev/`.
#   release    deploys X.Y. When X.Y is the highest version on gh-pages, counting X.Y itself
#              (is-latest-minor.sh over versions.json), it also moves the `latest` alias and the
#              default onto X.Y, and publishes the viewer into `viewer/` and llms.txt at the root;
#              otherwise it deploys only its own folder and leaves `latest`, the viewer and
#              llms.txt alone.
#
# mike and publish-viewer.sh (PUSH=local) only commit onto the local gh-pages branch; this
# script pushes it. By default every read of gh-pages is of the commit a fresh
# `git fetch origin gh-pages` returned, and every write runs through retry-gh-pages.sh, which
# resets the local gh-pages to that commit, then the write and its push run; gh-pages must
# already exist on origin, or the first fetch fails the run. With --no-push nothing is fetched
# or pushed, and the reads and the final checks are of the local branch, which may not exist yet.
#
# The push token: a push run reads it from file descriptor 3 (the workflow's GITHUB_TOKEN, which
# the docs.yml deploy step writes there and keeps out of this process's environment), keeps it in
# an unexported shell variable and closes the descriptor before anything else runs, so mike,
# mkdocs and every other command run without it, and no environment of theirs or of the
# processes above them holds it. Only `git push` gets it, as an Authorization header in that one
# command's environment (GIT_CONFIG_COUNT and friends): never in argv, .git/config or a file.
# A push run refuses when PAGES_PUSH_TOKEN is still in its environment, since the token would
# then reach mike, and when nothing arrives on the descriptor within 5 seconds. Tracing is
# switched off first, so `bash -x` cannot print it. --no-push never reads the descriptor.
#
# Runs from the repository root. Needs mike (and mkdocs) on the PATH, jq, base64, a git
# identity, the viewer's module in viewer/pkg (make wasm) and GITHUB_SHA, which names the
# viewer's commit.
# Usage: deploy-docs.sh dev | release X.Y 3<<<"$TOKEN"
#        deploy-docs.sh --no-push dev | release X.Y
{
  set +x
  token=""
  token_status=0
  if [ "${1:-}" != "--no-push" ]; then
    read -r -t 5 token <&3 || token_status=$?
  fi
  exec 3<&-
} 2> /dev/null
set -euo pipefail

usage() {
  echo "deploy-docs.sh: $1" >&2
  echo "usage: deploy-docs.sh [--no-push] dev | deploy-docs.sh [--no-push] release X.Y" >&2
  exit 2
}

scripts=$(cd "$(dirname "$0")" && pwd)
cd "$scripts/../.."

push=true
if [ "${1:-}" = "--no-push" ]; then
  push=false
  shift
fi
kind=${1:-}
version=""
case "$kind" in
  dev)
    [ $# -eq 1 ] || usage "dev takes no version, got: ${*:2}"
    ;;
  release)
    [ $# -eq 2 ] || usage "release takes exactly one version X.Y, got $(($# - 1)) arguments"
    version=$2
    [[ "$version" =~ ^[0-9]+\.[0-9]+$ ]] || usage "the release version must be X.Y, got $version"
    ;;
  *)
    usage "the kind must be dev or release, got '${kind}'"
    ;;
esac

if [ "$push" = true ]; then
  if [ -n "${PAGES_PUSH_TOKEN+set}" ]; then
    echo "deploy-docs: PAGES_PUSH_TOKEN is set in the environment, where mike would inherit it; the docs.yml deploy step starts this script as \`env -u PAGES_PUSH_TOKEN\` and writes the token to file descriptor 3" >&2
    exit 1
  fi
  if [ "$token_status" -gt 128 ]; then
    echo "deploy-docs: nothing arrived on file descriptor 3 within 5 seconds (the docs.yml deploy step writes the workflow's GITHUB_TOKEN there and closes it); --no-push builds without it" >&2
    exit 1
  fi
  if [ -z "$token" ]; then
    echo "deploy-docs: a push run reads its token from file descriptor 3, which is empty or closed (the docs.yml deploy step writes the workflow's GITHUB_TOKEN there); --no-push builds without it" >&2
    exit 1
  fi
  pages=""
  # shellcheck source-path=SCRIPTDIR source=retry-gh-pages.sh
  . "$scripts/retry-gh-pages.sh"
else
  pages=gh-pages
fi
# KIND and VERSION are read by publish-viewer.sh.
export NO_MKDOCS_2_WARNING=true KIND=$kind VERSION=$version

# Brings gh-pages up to date; a no-push run reads the local branch and fetches nothing. A push
# run reads the commit the fetch returned, not origin/gh-pages, which a single-branch or shallow
# clone may leave stale.
refresh() {
  if [ "$push" = true ]; then
    git fetch origin gh-pages
    pages=$(git rev-parse --verify --quiet 'FETCH_HEAD^{commit}') \
      || {
        echo "git fetch origin gh-pages did not produce a gh-pages commit in FETCH_HEAD" >&2
        exit 1
      }
  fi
}
# Pushes the local gh-pages to origin. The header holds the token, so it is set in this one
# command's environment; git's own messages name the remote, never the header. Client hooks are
# off for the push, so a hook an earlier build step planted in the checkout cannot read it.
push_pages() {
  local auth
  auth=$(printf 'x-access-token:%s' "$token" | base64 | tr -d '\n') && [ -n "$auth" ] || {
    echo "deploy-docs: could not encode the push token for the Authorization header (base64 failed); nothing was pushed" >&2
    return 1
  }
  GIT_CONFIG_COUNT=1 \
    GIT_CONFIG_KEY_0=http.https://github.com/.extraheader \
    GIT_CONFIG_VALUE_0="AUTHORIZATION: basic $auth" \
    git -c core.hooksPath=/dev/null push --no-verify origin gh-pages:gh-pages
}
# One attempt of a push run: the command commits onto the local gh-pages, then it is pushed.
# Chained with &&, because retry_gh_pages runs it where `set -e` does not apply.
commit_then_push() { "$@" && push_pages; }
# Runs a command that commits onto the local gh-pages: as is with --no-push, otherwise through
# retry_gh_pages, which pushes the commit and redoes both when the push is rejected.
write_pages() {
  if [ "$push" = true ]; then retry_gh_pages commit_then_push "$@"; else "$@"; fi
}
# The release deploys, as functions so that write_pages can retry them as one command.
deploy_latest() { mike deploy --update-aliases "$version" latest && mike set-default latest; }
on_pages() { git cat-file -e "$pages:$1" 2>/dev/null; }

# The viewers and llms.txt already published, which must still be there at the end.
refresh
release_before=false
dev_before=false
llms_before=false
if on_pages viewer/index.html; then release_before=true; fi
if on_pages viewer/dev/index.html; then dev_before=true; fi
if on_pages llms.txt; then llms_before=true; fi

moves=false
if [ "$kind" = "dev" ]; then
  write_pages mike deploy --title "dev (unreleased)" dev
else
  refresh
  moves=$({ git show "$pages:versions.json" 2>/dev/null || echo '[]'; } | "$scripts/is-latest-minor.sh" "$version")
  echo "Version $version: latest moves = $moves"
  if [ "$moves" = "true" ]; then
    write_pages deploy_latest
  else
    write_pages mike deploy "$version"
  fi
fi

# `viewer/` for a release that moves latest (keeping `viewer/dev/`), `viewer/dev/` for dev. A
# release also writes llms.txt at the root. Nothing else on gh-pages is touched.
if [ "$kind" = "dev" ] || [ "$moves" = "true" ]; then
  write_pages env PUSH=local "$scripts/publish-viewer.sh"
fi

# What the run was meant to publish.
refresh
has() { on_pages "$1" || { echo "gh-pages lacks $1"; exit 1; }; }
has versions.json
git show "$pages:versions.json" | jq -r '.[].version' | while read -r listed; do
  has "$listed/index.html"
done
# An alias is a symlink to its version's folder.
git show "$pages:versions.json" | jq -r '.[].aliases[]' | while read -r alias; do
  has "$alias"
done
# A release deploy must have published its own folder and, when latest was meant to move, the
# alias on it.
if [ "$kind" = "release" ]; then
  has "$version/index.html"
  if [ "$moves" = "true" ]; then
    git show "$pages:versions.json" | jq -e --arg v "$version" 'any(.[]; .version == $v and (.aliases | index("latest")))' > /dev/null \
      || { echo "gh-pages versions.json does not list latest among the aliases of $version"; exit 1; }
  fi
fi
# A viewer that was there before the run must still be there (mike does not touch it); this
# run's own viewer must have been published.
if { [ "$kind" = "release" ] && [ "$moves" = "true" ]; } || [ "$release_before" = "true" ]; then has viewer/index.html; has viewer/pkg/index.js; fi
if { [ "$kind" = "release" ] && [ "$moves" = "true" ]; } || [ "$llms_before" = "true" ]; then has llms.txt; fi
if [ "$kind" = "dev" ] || [ "$dev_before" = "true" ]; then has viewer/dev/index.html; has viewer/dev/pkg/index.js; fi
for dir in viewer viewer/dev; do
  if on_pages "$dir/pkg/oxedi_bg.wasm"; then echo "gh-pages holds $dir/pkg/oxedi_bg.wasm, which the page never loads"; exit 1; fi
done
# The published page must carry the policy that forbids network connections.
for page in viewer/index.html viewer/dev/index.html; do
  if on_pages "$page"; then
    git show "$pages:$page" | grep -q "connect-src 'none'" || { echo "gh-pages $page lacks the Content-Security-Policy (connect-src 'none')"; exit 1; }
  fi
done
echo "gh-pages ($pages) holds what the $kind deploy${version:+ of $version} was meant to publish."
