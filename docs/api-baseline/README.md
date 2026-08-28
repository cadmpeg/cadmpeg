# Public API baselines

Any commit that intentionally changes a crate's public surface regenerates that crate's snapshot in the same commit and adds a `[[change]]` row to `docs/public-api-ledger.toml`. `git diff docs/api-baseline/` is the API change record; the snapshot commit hash is recorded at the top of each file.

Regenerate a crate snapshot with nightly rustc and `cargo-public-api`. CI installs nightly and pins `cargo-public-api` 0.52.0. `-s` omits blanket impls (`Into`, `From`, `Any`).

```
SHORT=$(git rev-parse --short HEAD)
cargo +nightly public-api -p cadmpeg-core --color never -s \
  | { echo "# generated at $SHORT"; cat; } > docs/api-baseline/cadmpeg-core.txt
```

`scripts/check-public-api-ledger.py` checks that the ledger TOML parses, every `commit` field is a 40-character SHA and a known git object when history is present, and each snapshot file exists with a `# generated at` header. The staged-change check requires each crate named by an added `[[change]]` row to have its snapshot staged in the same commit. This fast structural check runs in the commit hook and does not require nightly tooling.

CI is the enforcing API-diff layer. It installs the pinned tooling and runs `scripts/check-public-api-ledger.py --skip-git-objects --diff --require-tooling`, so missing tooling or a stale snapshot fails. A local `--diff` without `--require-tooling` still prints one warning and passes when nightly or `cargo-public-api` is unavailable.
