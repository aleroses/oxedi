#!/usr/bin/env bash
# Commits the viewer (and, for a release, the site's llms.txt) onto a gh-pages commit: builds a
# worktree from it, replaces the viewer's folder and commits. Nothing outside viewer/ and, for a
# release, the root llms.txt is touched. It never pushes: deploy-docs.sh pushes the local
# gh-pages branch after it, with the token only that push gets.
# Environment: KIND (release or dev), VERSION (release only), GITHUB_SHA, and PUSH: false (the
# default) starts from the gh-pages commit it fetches and stops after the commit in the
# temporary worktree (a dry run); local starts from the local gh-pages branch, fetches nothing,
# and moves that branch onto the commit (deploy-docs.sh).
set -euo pipefail

push=${PUSH:-false}
case "$push" in
  local) base=gh-pages ;;
  false)
    git fetch origin gh-pages
    base=FETCH_HEAD
    ;;
  *)
    echo "publish-viewer: PUSH must be false or local, got '$push'; deploy-docs.sh pushes gh-pages" >&2
    exit 2
    ;;
esac
pages=$(mktemp -d)/gh-pages
git worktree add -B viewer-publish "$pages" "$base"
trap 'git worktree remove --force "$pages"' EXIT

# `viewer/` for a release (keeping `viewer/dev/`), `viewer/dev/` for master.
if [ "$KIND" = "release" ]; then
  dest="$pages/viewer"
  mkdir -p "$dest"
  find "$dest" -mindepth 1 -maxdepth 1 ! -name dev -exec rm -rf {} +
  label="$VERSION"
  # llms.txt at the site root follows `latest`, like the viewer; mike only copies it into each
  # version's folder.
  cp docs/llms.txt "$pages/llms.txt"
  paths="viewer llms.txt"
  what="Viewer and llms.txt"
else
  dest="$pages/viewer/dev"
  rm -rf "$dest"
  mkdir -p "$dest"
  label="dev"
  paths="viewer"
  what="Viewer"
fi
# The page embeds the module in pkg/module.js and never fetches the raw .wasm.
tar -C viewer --exclude=./tests --exclude=./pkg/oxedi_bg.wasm -cf - . | tar -C "$dest" -xf -
cd "$pages"
# shellcheck disable=SC2086
git add -A $paths
if git diff --cached --quiet; then
  echo "$what on gh-pages: already up to date."
else
  git commit -m "$what $label from ${GITHUB_SHA::7}"
  if [ "$push" = "local" ]; then
    git branch -f gh-pages HEAD
    echo "PUSH=local: moved the local gh-pages to $(git rev-parse --short HEAD)."
  else
    echo "PUSH=false: committed $(git rev-parse --short HEAD) locally, nothing pushed."
  fi
fi
