- Write dense, direct technical prose. No hype, filler, rhetorical framing, or emphasis cadence. Use ASD-STE100.
- Specs state the settled format model as fact. Never qualify it with corpus, sample, experiment, or provenance language.
- Specs contain byte semantics and invariants only. Put genuine unknowns in `docs/formats/*-open-items.md`.
- Do not treat finite evidence as an unknown. Do not put research history, project status, implementation bugs, or export behavior in specs.
- When moving code, update callers to import from the owning module. Do not retain old paths through top-level or orchestration re-exports.
- Commit early, commit often.

Multi-agent repository etiquette:

- One worktree and one branch per agent. Do not edit or build inside another agent's worktree.
- Unstaged changes you did not make belong to another agent. Do not commit them, revert them, or bypass hooks because of them.
- Use `--no-verify` only with the reason stated in the commit body.
- In a conflicted merge, restore a file from a merge stage with `scripts/restore-merge-stage.sh`, not with `git checkout` or `git restore`.

Build and test operations:

- Run several tests in one invocation with filters after the separator: `cargo test -- name_a name_b`. Plain `cargo test name_a name_b` fails with `unexpected argument`. Fast suite: `cargo test-fast`. Regenerate golden snapshots after an intended change: `UPDATE_GOLDEN=1 cargo test-fast golden`, then review the diff.
- Build and test with `-q`: `cargo build -q`, `cargo test -q` (`cargo test-fast` is quiet already). The flag removes the `Compiling` status lines and condenses the per-test list to one character per test; warnings, errors, and failure detail still print in full. A quiet build with exit status 0 is a completed build — do not rerun it verbosely to confirm. Use `--verbose` only when the compile plan itself is the question.
- The pre-commit gate scopes clippy and tests to the staged crates plus their workspace dependents. Triage a lint finding once and apply a targeted `#[allow]` with a comment; do not rerun the full gate against code you did not touch.
- The `JsonSchema` derives are behind a `schema` feature that is off by default in `cadmpeg-ir`, `cadmpeg-core`, `cadmpeg-codec-f3d`, `cadmpeg-codec-catia`, and `cadmpeg-codec-sldprt`. `cargo test-fast` does not build them. After changing an IR or native record type run `cargo test -p cadmpeg-ir --features schema --lib`. Do not add `JsonSchema` to a bare `derive` list; use `#[cfg_attr(feature = "schema", derive(JsonSchema))]`.
- Changes to `cadmpeg-ir` or `cadmpeg-core` fan out to every codec crate and diverge all goldens. Add struct fields through `Default` or constructor helpers and plan the fan-out before editing.
- A successful decode is not a valid IR. Run `cadmpeg validate` on decoder output; it enforces ID-naming and topology conventions beyond decode success.
- Test expectations come from the specification or approximate equality. Do not paste a failing run's observed output into an expectation.
- Rebuild the `cadmpeg` binary before a batch decode run. An unchanged report after a code change indicates a stale binary, not an ineffective change.
- Do not write file content back from a truncated read. Re-read a file after formatting hooks or context compaction before you patch it.
- Do byte-level work through `cadmpeg inspect` rather than `xxd`, `od`, `strings`, or `unzip`: `hex --offset --len`, `read --type --offset --count`, `find --hex|--ascii|--utf16le` with `--max 0` for every hit and `--context N` for bytes around each hit, plus `strings --min`, `struct --layout`, `diff`, and `container`. Extract a ZIP member with `cadmpeg inspect extract FILE MEMBER -o OUT`, not `unzip -p`: a bracketed name such as `Body[Active].brp` matches byte-exactly there, where `unzip` reads it as a glob and reports `filename not matched`.
- Fixed byte offsets for a format are in `docs/layouts/<format>.toml`, with a rendered `<format>.md` beside it. Read the table before you derive an offset again, and record a settled fixed-offset layout there, not only in specification prose. `cargo test -p cadmpeg --test layout_tables` checks the arithmetic and the specification citations.
- Project report and CADIR JSON with `cadmpeg query {summary,coverage,findings,losses,counts,item} FILE` (aliases `arenas` / `record`; `-` reads stdin) instead of exploratory `jq`: it detects the artifact kind and prints tab-separated rows (`item` prints pretty JSON). Use `query item FILE ARENA ID` for one record by string `id` (arena names match `query counts --json`; join by following `links` and running again). The raw paths, when `jq` is unavoidable: native arenas under `.native.<codec>.arenas`, findings under `.validation_report.findings`, losses under `.validation_report.losses` or `.decode_report.losses`, coverage under `.decode_report.coverage` (absent when empty). Field-predicate scans stay on jq — prefer guarded forms: `jq '.model.<arena>[]? | select(.id == "…")' file.json` and `.native.<codec>.arenas.<arena>[]?` (run `query counts` first; arena presence varies per document; parenthesize `select` pipelines). The printed error count and exit status 1 count the `error` and `blocking` findings.
- The CLI package is `cadmpeg`, so `cargo build -p cadmpeg`. There is no `cadmpeg-cli`. The package has no library target, so select its tests with `--all-targets`; `--lib` is an error.
- Give each file in a corpus-wide decode loop its own `timeout N`. One pathological input then cannot stall the whole run.
