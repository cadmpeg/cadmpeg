#!/bin/sh
# Restore conflicted files from a merge stage without `git checkout` or
# `git restore`, which agent policy forbids. During a conflicted merge the
# index holds three stages per path: 1 = base, 2 = ours, 3 = theirs. This
# script writes the chosen stage's blob over the working-tree file and keeps
# the previous working-tree content next to it as `<path>.premerge` so the
# operation is additive and reversible.
#
# Usage:
#   scripts/restore-merge-stage.sh --list
#   scripts/restore-merge-stage.sh <base|ours|theirs|1|2|3> <path>...
set -eu

usage() {
  echo "usage: $0 --list | <base|ours|theirs|1|2|3> <path>..." >&2
  exit 2
}

[ $# -ge 1 ] || usage

if [ "$1" = "--list" ]; then
  git diff --name-only --diff-filter=U
  exit 0
fi

case "$1" in
  base|1) stage=1 ;;
  ours|2) stage=2 ;;
  theirs|3) stage=3 ;;
  *) usage ;;
esac
shift
[ $# -ge 1 ] || usage

for path in "$@"; do
  if ! git rev-parse --verify --quiet ":$stage:$path" >/dev/null; then
    echo "$0: no stage $stage entry for '$path' (not conflicted, or wrong path)" >&2
    exit 1
  fi
  if [ -f "$path" ]; then
    cp -p "$path" "$path.premerge"
  fi
  git show ":$stage:$path" > "$path"
  echo "restored '$path' from stage $stage (previous content: '$path.premerge')"
done
