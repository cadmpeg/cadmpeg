#!/usr/bin/env bash
# Record decode timing and build-cost baselines for comparative regression checks.
#
# Output: docs/perf-baseline/<hostname>-<short-hash>/
#   decode-<codec>.json   hyperfine export per codec with file fixtures
#   build.txt             /usr/bin/time graphs + target size
#
# Requires: hyperfine on PATH. Rebuilds target/release/cadmpeg before timing.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "error: hyperfine not on PATH (install: brew install hyperfine)" >&2
  exit 1
fi

HOST="$(hostname -s | tr '[:upper:]' '[:lower:]')"
SHORT_HASH="$(git rev-parse --short HEAD)"
OUT="docs/perf-baseline/${HOST}-${SHORT_HASH}"
mkdir -p "$OUT"

echo "==> release binary (never benchmark a stale cadmpeg)"
cargo build -q --release -p cadmpeg
CADMPEG="target/release/cadmpeg"
test -x "$CADMPEG"

# Largest regular file under a directory (byte size). Empty dir → empty string.
largest_fixture() {
  local dir="$1"
  local best="" best_size=-1 size path
  while IFS= read -r -d '' path; do
    size="$(wc -c <"$path" | tr -d ' ')"
    if [ "$size" -gt "$best_size" ]; then
      best_size="$size"
      best="$path"
    fi
  done < <(find "$dir" -type f -print0 2>/dev/null)
  printf '%s' "$best"
}

# Map a codec crate directory name → fixture directory with committed file inputs.
# Prints the path, or nothing when the codec has no file fixtures.
fixture_dir_for() {
  local crate_dir="$1"
  local name
  name="$(basename "$crate_dir")"
  local golden="$crate_dir/tests/golden/fixtures"
  local step_fixtures="$crate_dir/tests/fixtures"
  local freecad_corpus="$ROOT/corpus/freecad_fcstd/fixtures"

  if [ -d "$golden" ] && [ -n "$(find "$golden" -type f -print -quit 2>/dev/null)" ]; then
    printf '%s' "$golden"
    return
  fi
  if [ "$name" = "cadmpeg-codec-step" ] && [ -d "$step_fixtures" ] \
    && [ -n "$(find "$step_fixtures" -type f -print -quit 2>/dev/null)" ]; then
    printf '%s' "$step_fixtures"
    return
  fi
  if [ "$name" = "cadmpeg-codec-freecad" ] && [ -d "$freecad_corpus" ] \
    && [ -n "$(find "$freecad_corpus" -type f -print -quit 2>/dev/null)" ]; then
    printf '%s' "$freecad_corpus"
    return
  fi
}

echo "==> decode timing (largest fixture per codec with file fixtures)"
{
  echo "# decode fixture selection"
  echo "# generated at $SHORT_HASH on $HOST"
  echo
} >"$OUT/decode-selection.txt"

for crate_dir in "$ROOT"/crates/cadmpeg-codec-*; do
  [ -d "$crate_dir" ] || continue
  codec="$(basename "$crate_dir" | sed 's/^cadmpeg-codec-//')"

  if [ "$codec" = "sat" ]; then
    echo "skip sat: no committed file fixtures (code-built inputs only)" | tee -a "$OUT/decode-selection.txt"
    continue
  fi

  fixtures="$(fixture_dir_for "$crate_dir")"
  if [ -z "$fixtures" ]; then
    echo "skip $codec: no committed file fixtures" | tee -a "$OUT/decode-selection.txt"
    continue
  fi

  fixture="$(largest_fixture "$fixtures")"
  if [ -z "$fixture" ]; then
    echo "skip $codec: fixture dir empty ($fixtures)" | tee -a "$OUT/decode-selection.txt"
    continue
  fi

  size="$(wc -c <"$fixture" | tr -d ' ')"
  rel="${fixture#"$ROOT"/}"
  echo "$codec	$size	$rel" | tee -a "$OUT/decode-selection.txt"
  echo "timing $codec ($(basename "$fixture"), ${size} bytes)"
  # Write CADIR to stdout (not -o /dev/null): the CLI stages a temp file next to
  # -o, and /dev is not writable. Discard stdout so the timed path is dump only.
  hyperfine --warmup 1 --runs 5 --export-json "$OUT/decode-${codec}.json" \
    "timeout 120 '$CADMPEG' dump '$fixture' >/dev/null"
done

echo "==> build metrics (cadmpeg-codec-iges representative + CLI + test-fast)"
BUILD_LOG="$OUT/build.txt"
{
  echo "# build metrics"
  echo "# generated at $SHORT_HASH on $HOST"
  echo "# /usr/bin/time -p around each cargo invocation"
  echo
} >"$BUILD_LOG"

run_timed() {
  local label="$1"
  shift
  {
    echo "## $label"
    echo "\$ $*"
    /usr/bin/time -p "$@"
    echo
  } >>"$BUILD_LOG" 2>&1
}

cargo clean -p cadmpeg-codec-iges
run_timed "clean build cadmpeg-codec-iges" \
  cargo build -q -p cadmpeg-codec-iges

run_timed "incremental no-op build cadmpeg-codec-iges" \
  cargo build -q -p cadmpeg-codec-iges

cargo clean -p cadmpeg-codec-iges
run_timed "clean test compile cadmpeg-codec-iges --no-run" \
  cargo test -q -p cadmpeg-codec-iges --no-run

cargo clean -p cadmpeg
run_timed "clean build cadmpeg CLI" \
  cargo build -q -p cadmpeg

run_timed "test-fast --no-run" \
  cargo test-fast --no-run

{
  echo "## target size"
  du -sh target
} >>"$BUILD_LOG" 2>&1

echo "==> wrote $OUT"
ls -la "$OUT"
