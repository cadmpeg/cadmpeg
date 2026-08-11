# Public API baselines

Any commit that intentionally changes a crate's public surface regenerates that crate's snapshot in the same commit, so `git diff docs/api-baseline/` is the API change record; the snapshot commit hash is recorded at the top of each file.
