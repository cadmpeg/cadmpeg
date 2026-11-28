A task brief or issue that states its own rule for its work takes precedence over this file.

cadmpeg decodes as much of each file as current format knowledge allows. When the format allows more than one reading, the decoder applies the most likely reading and charges a loss code that names the assumption. Dropping a value that the most likely reading decodes is a regression, also when a loss code reports the drop.

Writing:

- Write dense, direct technical prose in ASD-STE100. No hype, filler, rhetorical framing, or emphasis.
- State each fact once, affirmatively. Never define by negation: no "X, not Y" doublets, no "only X ... never Y" pairs, no "the spec gives/states/names no ..." tails. The open item's existence already records the gap; a Known paragraph that ends with the code behavior needs no closing "no rule" sentence.
  - Bad: "the value is observed, not required"; "reads the word only into `outer_version` and never compares it"; "`sldprt.md` §8 gives no prose for reused class tags".
  - Good: "every value decodes"; "the decoder stores the word in `outer_version`"; cut the tail.
- Specifications state the settled format model as fact: byte semantics and invariants only. They carry no corpus, sample, experiment, or provenance qualifiers, and no research history, project status, implementation bugs, or export behavior.
- Genuine unknowns go in `docs/formats/*-open-items.md`, with the fields Question, Known, Need, Conflict, and Note only. Finite evidence is not an unknown. When an item is settled, write the answer into the specification and delete the item in the same change.
- An open item is self-contained. A reader who holds no CAD file must understand what is known, what is unknown, and what evidence settles it. Write the Known as a description of the stored form: the record or frame, its length, its fields and their offsets, the values they take, and how its records relate to each other. A fact that came from one file is still a fact about a form; state the form.
  - Do not send the reader to a file. Name no file, design, or sample, and do not describe one in place of its name ("one public design", "a pulley model"). A description is the same reference with the name removed.
  - Do not locate a fact by a value that has meaning only inside one file: an absolute record index, a stream offset, an entity identity, or a feature label such as "Extrude 3". State the relation that the values show ("the profile group is the record after the scope"), and state it only when the specification, the layout table, or the decoder confirms it. Without that confirmation, give the stored values as an instance of the form and leave the locator out.
  - Do not count files or records as evidence ("in 70 designs", "nine such scopes"). State what the form stores and which values it takes.
  - Write the Need as the save that would settle the question: the model to build, the control to change, and the records to compare.
  - Bad: "In `<name>.f3d`, C-Pattern 10 stores a 220-byte frame at record 4197"; the same sentence with "a public pulley design" in place of the name.
  - Good: "A 220-byte frame stores control `7` at offset 89 and three consecutive marked references at offsets 97, 108, and 119."
- Text in this repository refers only to CAD files committed under `corpus/`. This holds for specifications, open items, code, tests, commit messages, pull request titles and descriptions, and review comments. Each author applies it to the text they write.
- Fixed byte offsets live in `docs/layouts/<format>.toml`. Read the table before you derive an offset, and record a settled fixed-offset layout there. `docs/layouts/README.md` gives the check and regeneration commands.

Code:

- A comment states only intent that the code cannot show. Do not use comments to hedge, justify, narrate history, or record invariants; enforce an invariant with code or a test.
- When you move code, update callers to import from the owning module. Do not keep old paths through re-exports.
- Codec crate roots are facades. Their `pub` names must match `docs/codec-facade.toml` (`scripts/check-codec-facade.py`). Read-only codecs have no encoder.
- Read file bytes through `cadmpeg_core::decode::View` (typed reads, `req_*`, offset helpers such as `View::u16_le_at`). Keep a `View`, not `&[u8]`, where its `SpaceId` must survive. Propagate `req_*` errors with `?` through a codec-local `From<ParseError>` so the location survives. Validate a parsed count with `View::counted` or `bounded_len`, then allocate with `DecodeContext::alloc_filled`.
- Each codec's loss vocabulary is the `*LossCode` enum in `src/loss.rs`. Construct a `LossNote` only with `code.note(message)`. A new drop path adds a variant. The `family.detail` strings are the contract.
- Every validation finding names its rule: a message is `rule!("<area>.<detail>", "…")` and a `Finding { .. }` literal states `rule: rule_name!("…")`. The name is the contract; keep it when the message changes. `python3 scripts/name-validation-rules.py --write` names new sites (after a merge that conflicts in a validator file, take the incoming file and run it with `--reuse-from HEAD`). A decoder admission route lists the rules it reads in `validate/admit.rs`; a new rule joins a route only by a deliberate edit there.
- Dialect identity and capability come only from `crates/cadmpeg-registry/docs/dialects.toml` and `dialect-support.toml`. Do not add a dialect table to Rust. Regenerate codec id constants with `python3 scripts/check-dialects.py --write-generated`. The output dialect is selected only by `--to FORMAT[:DIALECT]`; do not add per-format target flags.
- CADIR, the decode sidecar, and the CLI report accept exactly one `IR_VERSION` and carry no migrations. Maintainers select `IR_VERSION` for a release. Keep V11 across independent PRs: `main` carries the unreleased wire `"11"` until its release; change the version only when the task explicitly requests it. Regenerate fixtures after a wire change.
- A change to `cadmpeg-ir` or `cadmpeg-core` reaches every codec and every golden. Add struct fields through `Default` or constructor helpers.
- `JsonSchema` derives use `#[cfg_attr(feature = "schema", derive(JsonSchema))]`. Only the `schema` feature builds them, and no dependent enables it: schema readers use the committed `crates/cadmpeg-ir/schema/*.json`. After changing an IR or native record type, run `UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema-order --test committed_schema` (it writes each object's `properties` in declared order; run no other test under `schema-order`, which changes the order of every `serde_json::Map`) and `python3 scripts/generate-annex-a.py --write`, then review the schema and annex diff.
- `python3 scripts/check-source-policy.py` enforces `docs/source-policy.md`: no production `#[allow]` or `#[expect]` (fix the lint), named tolerance constants, checked vector repeats, endian exceptions, test placement, and file-size limits. When you name a tolerance, never merge two distinct values into one constant. To change a rule, edit the rule and its tests.

Tests:

- Expectations come from the specification or from approximate equality, never from a failing run's output.
- Route a test by the owner of the invariant it asserts. Unit tests sit with their production owner: an inline `mod tests`, `foo/tests.rs`, or `foo/tests/mod.rs` with semantically named children, never numbered names. Integration tests (codec-wide decode, encode, or round trip; multi-module composition; the crate facade) go in `src/integration_tests.rs` or `src/integration_tests/`. Test support goes in `src/test_support.rs` or `src/test_support/`, and shared golden and round-trip helpers in `cadmpeg-test-support`; add no other support crate. Golden tests go in `src/golden_tests.rs` or `src/golden_tests/`. A crate root declares only these three test modules.
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

- Pull requests are squash-merged. The ticked `Signed off` box in the pull request template is the DCO sign-off for the whole pull request. Do not add a `Signed-off-by` line, and do not require or add per-commit sign-offs.
- `main` has no merge queue, and pull requests merge without a rebase onto the current `main`. Two pull requests that pass CI separately can break `main` together. Fix a broken `main` with a small forward-fix pull request.

Shared checkouts:

- Work in your assigned worktree and branch.
- Changes you did not make belong to another agent. Do not commit, revert, or unstage them. Check `git diff --cached --name-only` before you commit.
- Use `--no-verify` only with the reason in the commit body.
- In a conflicted merge, restore a file from a merge stage with `scripts/restore-merge-stage.sh`.
