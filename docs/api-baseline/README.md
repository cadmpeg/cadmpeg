# Public API baselines

Any commit that intentionally changes a crate's public surface regenerates that crate's snapshot in the same commit. `git diff docs/api-baseline/` is the API change record; the snapshot commit hash is recorded at the top of each file. `docs/public-api-ledger.toml` holds only the header (`baseline_commit`, `api_baseline_dir`, `measured_at`); it carries no per-change rows.

Regenerate a crate snapshot with nightly rustc and `cargo-public-api`. CI installs nightly and pins `cargo-public-api` 0.52.0. `-sss` omits blanket impls, auto-trait impls, and auto-derived impls.

```
SHORT=$(git rev-parse --short HEAD)
cargo +nightly public-api -p cadmpeg-core --color never -sss \
  | { echo "# generated at $SHORT"; cat; } > docs/api-baseline/cadmpeg-core.txt
```

`scripts/check-public-api-ledger.py` checks that the ledger TOML parses, its commit fields are 40-character SHAs and known git objects when history is present, and each snapshot file exists with a `# generated at` header. This fast structural check runs in the commit hook and does not require nightly tooling.

CI is the enforcing API-diff layer. It installs the pinned tooling and runs `scripts/check-public-api-ledger.py --skip-git-objects --diff --require-tooling`, so missing tooling or a stale snapshot fails. A local `--diff` without `--require-tooling` still prints one warning and passes when nightly or `cargo-public-api` is unavailable.
