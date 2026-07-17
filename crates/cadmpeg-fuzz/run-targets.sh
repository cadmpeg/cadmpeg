#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Run a list of fuzz targets (one per line on stdin) for FUZZ_TIME seconds
# each, seeded from the committed corpus and dictionaries so magic-gated
# container and leaf targets reach real decode paths instead of burning the
# whole budget failing the format preamble from an empty corpus.
#
# Every target runs even if an earlier one crashes or times out: failures are
# collected and reported together, so a crash in an early-alphabet target does
# not hide independent regressions in later targets. The script exits non-zero
# if any target failed.
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fuzz_dir="crates/cadmpeg-fuzz"
time_budget="${FUZZ_TIME:-60}"

failures=()
while IFS= read -r target; do
  target="$(echo "$target" | tr -d '[:space:]')"
  [ -z "$target" ] && continue
  # `cargo fuzz list` also prints the seed-generator bins this package
  # declares; they are Cargo utilities, not fuzz targets, so skip them.
  case "$target" in generate_*) continue ;; esac

  # Seed corpus: the checked-in directory named for the target, when present.
  corpus=()
  if [ -d "$here/seeds/$target" ]; then
    corpus+=("$fuzz_dir/seeds/$target")
  fi

  # libFuzzer options: a per-target dictionary when one is committed, plus the
  # wall-clock bound.
  opts=("-max_total_time=$time_budget")
  if [ -f "$here/dictionaries/$target.dict" ]; then
    opts+=("-dict=$fuzz_dir/dictionaries/$target.dict")
  fi

  echo "::group::fuzz $target (${time_budget}s, corpus=${corpus[*]:-none})"
  if cargo +nightly fuzz run "$target" --fuzz-dir "$fuzz_dir" \
      "${corpus[@]}" -- "${opts[@]}"; then
    echo "$target: ok"
  else
    echo "$target: FAILED"
    failures+=("$target")
  fi
  echo "::endgroup::"
done

if [ "${#failures[@]}" -ne 0 ]; then
  echo "fuzz failures: ${failures[*]}"
  exit 1
fi
echo "all fuzz targets passed"
