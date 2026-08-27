# Public API baselines

Any commit that intentionally changes a crate's public surface regenerates that crate's snapshot in the same commit and adds a `[[change]]` row to `docs/public-api-ledger.toml`. `git diff docs/api-baseline/` is the API change record; the snapshot commit hash is recorded at the top of each file.

Regenerate a crate snapshot. Requires `cargo-public-api` and nightly rustc; CI does not install them. `-s` omits blanket impls (`Into`, `From`, `Any`).

```
SHORT=$(git rev-parse --short HEAD)
cargo +nightly public-api -p cadmpeg-core --color never -s \
  | { echo "# generated at $SHORT"; cat; } > docs/api-baseline/cadmpeg-core.txt
```

`scripts/check-public-api-ledger.py` checks that the ledger TOML parses, every `commit` field is a 40-character SHA and a known git object when history is present, and each snapshot file exists with a `# generated at` header. The staged-change check requires each crate named by an added `[[change]]` row to have its snapshot staged in the same commit. `--diff` regenerates and compares snapshots for crates with staged source changes; with an empty index it compares all snapshots. The diff check prints one warning and passes when nightly or `cargo-public-api` is unavailable.
