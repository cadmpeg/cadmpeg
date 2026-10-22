#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Build the merge base with origin/main, isolated from the current checkout.
# On success stdout contains a shell export for CADMPEG_BASE_BIN. GitHub Actions
# also receives that variable through GITHUB_ENV for subsequent steps.
set -euo pipefail

if (( $# != 0 )); then
  echo "usage: scripts/base-build.sh (base is git merge-base HEAD origin/main)" >&2
  exit 2
fi

base_repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
base_commit="$(git -C "$base_repo" merge-base HEAD origin/main)"
base_source="$base_repo/target/base-src"
base_target="$base_repo/target/base-build"
base_install="$base_repo/target/base"
base_binary="$base_install/cadmpeg"
base_lock="$base_repo/target/base-build.lock"
base_temporary=""
mkdir -p "$base_repo/target"
if ! mkdir "$base_lock"; then
  echo "base build already owns $base_lock; do not build concurrently" >&2
  exit 1
fi
base_cleanup() {
  if [[ -n "$base_temporary" ]]; then
    rm -f -- "$base_temporary"
  fi
  rmdir -- "$base_lock"
}
trap base_cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [[ -e "$base_source" ]]; then
  if [[ "$(git -C "$base_source" rev-parse --show-toplevel)" != "$base_source" ]]; then
    echo "base source path is not its own worktree: $base_source" >&2
    exit 1
  fi
  if [[ "$(git -C "$base_source" rev-parse --path-format=absolute --git-common-dir)" != "$(git -C "$base_repo" rev-parse --path-format=absolute --git-common-dir)" ]]; then
    echo "base source belongs to a different repository: $base_source" >&2
    exit 1
  fi
  if [[ -n "$(git -C "$base_source" status --porcelain --untracked-files=normal)" ]]; then
    echo "base source worktree has local changes: $base_source" >&2
    exit 1
  fi
  git -C "$base_source" checkout --detach "$base_commit" >&2
else
  git -C "$base_repo" worktree add --detach "$base_source" "$base_commit" >&2
fi

mkdir -p "$base_install"
# A failed rebuild must not leave a previous base executable available as if
# it were the binary this invocation was asked to build.
rm -f -- "$base_binary" "$base_install/base.commit"
(
  cd "$base_source"
  CARGO_TARGET_DIR="$base_target" cargo build -q --release -p cadmpeg --bin cadmpeg
)
if [[ "$(git -C "$base_source" rev-parse HEAD)" != "$base_commit" ]] || [[ -n "$(git -C "$base_source" status --porcelain --untracked-files=normal)" ]]; then
  echo "base source changed during its build: $base_source" >&2
  exit 1
fi
base_built="$base_target/release/cadmpeg"
test -x "$base_built"
base_temporary="$(mktemp "$base_install/.cadmpeg.XXXXXX")"
cp -- "$base_built" "$base_temporary"
chmod +x "$base_temporary"
mv -- "$base_temporary" "$base_binary"
base_temporary=""
printf '%s\n' "$base_commit" > "$base_install/base.commit"
if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'CADMPEG_BASE_BIN=%s\n' "$base_binary" >> "$GITHUB_ENV"
fi
printf 'export CADMPEG_BASE_BIN=%q\n' "$base_binary"
