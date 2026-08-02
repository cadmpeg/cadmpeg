- Write dense, direct technical prose. No hype, filler, rhetorical framing, or emphasis cadence. Use ASD-STE100.
- Specs state the settled format model as fact. Never qualify it with corpus, sample, experiment, or provenance language.
- Specs contain byte semantics and invariants only. Put genuine unknowns in `docs/formats/*-open-items.md`.
- Do not treat finite evidence as an unknown. Do not put research history, project status, implementation bugs, or export behavior in specs.
- When moving code, update callers to import from the owning module. Do not retain old paths through top-level or orchestration re-exports.
- Commit early, commit often.

Multi-agent repository etiquette:

- One worktree and one branch per agent. Do not edit or build inside another agent's worktree.
- Stage explicit paths only. Do not use `git add -A`, `git add .`, or `git commit -a`; they capture other agents' unstaged work.
- Unstaged changes you did not make belong to another agent. Do not commit them, revert them, or bypass hooks because of them.
- Use `--no-verify` only with the reason stated in the commit body.
- In a conflicted merge, restore a file from a merge stage with `scripts/restore-merge-stage.sh`, not with `git checkout` or `git restore`.
- When several agents build concurrently, isolate build artifacts: set a per-worktree `CARGO_TARGET_DIR`, or export `RUSTFLAGS="-C metadata=$(git branch --show-current)"` so a shared target directory stays collision-free.

Build and test operations:

- Run several tests in one invocation: `cargo test name_a name_b`. Fast suite: `cargo test-fast`. Regenerate golden snapshots after an intended change: `UPDATE_GOLDEN=1 cargo test-fast golden`, then review the diff.
- The pre-commit gate scopes clippy and tests to the staged crates plus their workspace dependents. Triage a lint finding once and apply a targeted `#[allow]` with a comment; do not rerun the full gate against code you did not touch.
- Changes to `cadmpeg-ir` or `cadmpeg-codec-core` fan out to every codec crate and diverge all goldens. Add struct fields through `Default` or constructor helpers and plan the fan-out before editing.
- A successful decode is not a valid IR. Run `cadmpeg validate` on decoder output; it enforces ID-naming and topology conventions beyond decode success.
- Test expectations come from the specification or approximate equality. Do not paste a failing run's observed output into an expectation.
- Rebuild the `cadmpeg` binary before a batch decode run. An unchanged report after a code change indicates a stale binary, not an ineffective change.
- Do not write file content back from a truncated read. Re-read a file after formatting hooks or context compaction before you patch it.
- `/tmp` has a small quota. Put large scratch artifacts under `~/side2/tmp`.
