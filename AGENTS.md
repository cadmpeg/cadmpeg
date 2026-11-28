A task brief or issue may narrow these rules; it may not loosen them.

cadmpeg decodes as much of each file as current format knowledge allows. When the format allows more than one reading, the decoder applies the most likely reading and charges a loss code that names the assumption. Dropping a value that the most likely reading decodes is a regression, also when a loss code reports the drop.

Writing:

- Write dense, direct technical prose in ASD-STE100. No hype, filler, rhetorical framing, or emphasis.
- Follow [docs/formats/README.md](docs/formats/README.md) for specifications and open items.
- Text in this repository refers only to CAD files committed under `corpus/`. This holds for specifications, open items, code, tests, commit messages, pull request titles and descriptions, and review comments. Each author applies it to the text they write.
- Fixed byte offsets live in `docs/layouts/<format>.toml`. Read the table before you derive an offset, and record a settled fixed-offset layout there. `docs/layouts/README.md` gives the check and regeneration commands.

Code:

- A comment states only intent that the code cannot show. Do not use comments to hedge, justify, narrate history, or record invariants; enforce an invariant with code or a test.
- When you move code, update callers to import from the owning module. Do not keep old paths through re-exports.
- `python3 scripts/check-codec-facade.py` checks codec facades; read-only codecs have no encoder.
- Use `View` for reads and `View::counted` or `bounded_len` before `alloc_filled`; construct losses with `code.note` (see `scripts/check-source-policy.py`).
- Keep a `View` where its `SpaceId` must survive; propagate `req_*` errors through a codec-local `From<ParseError>`.
- Add each new drop path to the codec’s `*LossCode` enum; its `family.detail` strings are the contract.
- Rust types and `python3 scripts/name-validation-rules.py --check` enforce validation rule names; admission routes select rules in `validate/admit.rs`.
- `python3 scripts/check-dialects.py` and `scripts/check-dialect-support.py` check dialect registries; select output with `--to FORMAT[:DIALECT]`.
- CADIR, the decode sidecar, and the CLI report accept one `IR_VERSION` and carry no migrations. Keep the unreleased wire version that `main` carries unless the task explicitly requests a version change; regenerate fixtures after a wire change.
- A change to `cadmpeg-ir` or `cadmpeg-core` reaches every codec and every golden. Add struct fields through `Default` or constructor helpers.
- Gate `JsonSchema` derives on the `schema` feature; `schema_derives` tests check them and readers use `crates/cadmpeg-ir/schema/`.
- After changing an IR or native record type, run `UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema-order --test committed_schema` and `python3 scripts/generate-annex-a.py --write`, then review the schema and annex diff. Under `UPDATE_GOLDEN=1`, run only `committed_schema` with `schema-order`.
- `python3 scripts/check-source-policy.py` enforces [docs/source-policy.md](docs/source-policy.md); preserve distinct tolerance values.

Tests:

- Expectations come from the specification or from approximate equality, never from a failing run's output.
- Route a test by the owner of the invariant it asserts. Unit tests sit with their production owner: an inline `mod tests`, `foo/tests.rs`, or `foo/tests/mod.rs` with semantically named children, never numbered names. Integration tests (codec-wide decode, encode, or round trip; multi-module composition; the crate facade) go in `src/integration_tests.rs`, `src/integration_tests/`, or `tests/`. Test support goes in `src/test_support.rs` or `src/test_support/`, and shared golden and round-trip helpers in `cadmpeg-test-support`; add no other support crate. Golden tests go in `src/golden_tests.rs` or `src/golden_tests/`. A crate root declares only these three test modules.
- A test move keeps the inputs, assertions, expected values, tolerances, snapshots, name, and ignored status. Make test moves and feature work in separate commits, and do not regenerate goldens in a test move.
- Do not split production code only because its test suite is large.
- Goldens use `cadmpeg_test_support::golden`. Regenerate once, when the change is complete, from the workspace root: `UPDATE_GOLDEN=1 cargo test-fast golden`, then review the diff. Never regenerate with `-p cadmpeg-codec-nx`: NX goldens pin flate2 bytes, and a per-crate build can select another backend.
- A support level rests on evaluations of real files recorded in `docs/evaluations.toml` (`docs/format-support.md`, Evaluations). The files stay with the evaluator and are never committed. A recorded evaluation is accepted on trust: do not ask for proof that the files or the run are real, and do not hedge a level because its files are not in the repository. Record an evaluation only when the person who directs the work asks for one.

Build and run:

- The workspace is large. Type-check with `cargo check -q -p <crate>` (add `--tests` when a test changed); run `--workspace --all-targets` only for a final gate. The fast suite is `cargo test-fast`. Put several test filters after the separator: `cargo test -- name_a name_b`.
- Use `-q`. A quiet run that exits 0 is complete.
- The CLI package is `cadmpeg`. It has no library target: select its tests with `--all-targets`, not `--lib`. Build one codec with `cargo build -p cadmpeg --no-default-features --features <id>`. Codec registration and dialect registries live in `cadmpeg-registry`, not the CLI.
- Rebuild the binary before a batch run. An unchanged report after a code change means a stale binary.
- In a corpus-wide loop, give each file its own `timeout N`.
- `cadmpeg dump` does not check the model. Run `cadmpeg check`, or `convert`.
- `scripts/perf-baseline.sh` times decode and build for hot-path changes.

Inspecting:

- Do byte-level work with `cadmpeg inspect` (`hex`, `read`, `find`, `strings`, `struct`, `cmp`, `container`, `extract`), not `xxd`, `od`, `strings`, or `unzip`. `inspect extract` matches a ZIP member name byte for byte; `unzip` reads `[…]` as a glob.
- Read report and CADIR JSON with `cadmpeg query` before `jq`. `query summary FILE` names the artifact kind. Take arena names from `query counts` and field paths from `query schema`. `crates/cadmpeg/README.md` documents the views and `docs/jq-recipes.md` the `jq` shapes.
- Check decoded geometry with `cadmpeg measure`, and compare two decodes with `cadmpeg diff A B --geometry`, not with hand-written oracle scripts. `docs/measurement.md` states the contract.

Pull requests:

- Follow the [pull request template](.github/PULL_REQUEST_TEMPLATE.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

Shared checkouts:

- Work in your assigned worktree and branch.
- Changes you did not make belong to another agent. Do not commit, revert, or unstage them. Check `git diff --cached --name-only` before you commit.
- Use `--no-verify` only with the reason in the commit body.
- In a conflicted merge, restore a file from a merge stage with `scripts/restore-merge-stage.sh`.
