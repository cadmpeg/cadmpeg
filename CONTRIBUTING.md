# Contributing to cadmpeg

Read [LEGAL.md](LEGAL.md) before contributing. It defines where format knowledge may and may not come from, and it is binding.

---

## Two hard requirements

### 1. DCO sign-off

Sign off using the [pull request template](.github/PULL_REQUEST_TEMPLATE.md).

### 2. Provenance declaration on decoder and spec PRs

Any PR that adds or changes a **decoder** or a **format specification** must include this sentence, verbatim and truthful, in its description (see [LEGAL.md](LEGAL.md) for why):

> I derived this contribution solely by analyzing CAD files I am legally entitled to possess and from publicly available information. I did not use vendor SDKs, decompiled binaries, confidential or NDA-covered material, or any source listed as forbidden in LEGAL.md.

PRs that only touch tooling, the IR, exporters, tests, or non-specification documentation do not need the provenance declaration (but still need the DCO sign-off).

---

## Licensing of contributions

By contributing, you license code under **Apache-2.0** and documentation/spec content under **CC-BY-4.0**, matching the split described in the [README](README.md). No separate CLA is required. The DCO sign-off asserts your right to contribute.

---

## Ways to contribute

### Contribute a codec (turn a spec into a decoder)

A codec takes native bytes and produces the cadmpeg IR.

1. **Start from a spec.** Pick a format in [`docs/formats/`](docs/formats/) and review its companion `*-open-items.md` file. When a format allows multiple readings, implement the most likely reading and report the assumption with a loss code.
2. **Implement the codec interface.** Codecs implement the [`CodecBackend` trait in `crates/cadmpeg-ir/src/codec.rs`](crates/cadmpeg-ir/src/codec.rs), which defines `id`, `detect`, `inspect`, and `decode`. See [docs/architecture.md](docs/architecture.md) for the plugin model.
3. **Classify exactness correctly.** `ByteExact` means read from the source stream without transformation beyond documented unit conversion. `Derived` means computed deterministically from byte-exact inputs. `Inferred` means supplied from context or convention rather than an explicit source field. `Unknown` means origin or trustworthiness could not be established.
4. **Account for loss.** If your decoder skips or approximates something, surface it; do not silently drop bytes.
5. **Add authorized fixtures.** Add public CAD fixtures through the [corpus donation pipeline](corpus/README.md), not as untracked test binaries. NIST MBE PMI specimens are a separate maintainer-curated track (`LicenseRef-NIST-PD`); see [corpus/nist-pmi/README.md](corpus/nist-pmi/README.md). Add focused decode, validation, and round-trip tests where applicable.
6. **Include the provenance declaration** in your PR.

### Contribute format research (extend a spec)

Format research starts with a hypothesis about an unknown file region. Test that hypothesis against files you authored with controlled variations. Use the result to establish the stored form.

1. Work in the relevant [`docs/formats/`](docs/formats/) spec.
2. State settled byte semantics and invariants in the specification.
3. Follow the [format authoring guide](docs/formats/README.md) for specifications and open items.
4. Include the provenance declaration; format research is derived-knowledge work and the clean-room rules apply in full.

### Everything else

IR schema tooling, validators, exporters, corpus tooling, hex-annotation tooling, CLI ergonomics, and docs fixes offer other ways to contribute. See the [roadmap contributor entry points](docs/roadmap.md#contributor-entry-points) for bounded ways to contribute.

---

## Local CI gate

Install the pre-commit hook with `git config core.hooksPath .githooks`. Run the stable CI gate from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test-fast
cargo test --workspace --doc
```

See [AGENTS.md](AGENTS.md) for single-codec builds. A CLI test that needs a codec that is not built is compiled out.

Lint levels live in `[workspace.lints]` in the root `Cargo.toml`, so `cargo clippy` needs no trailing lint flags. Do not add them back: trailing lint flags are part of the compilation fingerprint, so a flagged run and a bare run evict each other's cached artifacts, and an editor running bare `cargo check` or `cargo clippy` then re-checks the workspace on every alternation.

Manifest levels apply to every cargo command, so a warning fails `cargo build` and `cargo test`, not only `cargo clippy`. Jobs on an unpinned toolchain set `RUSTFLAGS: -A warnings`, because lint sets differ between releases; the MSRV job is the only one.

CI builds test binaries without debug information. A failure there reports a backtrace without `file:line`; run the failing test locally to get the line numbers back.

PR CI tests only the packages a change reaches ([`scripts/ci_scope.py`](scripts/ci_scope.py)). A draft PR runs static checks, clippy, and the dependency audit when selected. macOS and Windows run in the nightly full gate, and on a `main` run that selects the full gate, which includes every release bump. To run them on a branch, dispatch CI on it: `gh workflow run ci.yml --ref BRANCH`. CodeQL analyzes Rust and Python on its schedule and on dispatch. To run it on a branch, dispatch CodeQL: `gh workflow run codeql.yml --ref BRANCH`.

A PR merges directly after its own CI run passes. Main can move between that run and the merge, so the main run is the first test of the merged tree. Fix a broken main with a small forward-fix pull request. A push to main queues behind an in-progress push run and replaces any queued push run. Each main run tests the packages that changed since the last successful main run, so the commits of a cancelled or failed run stay in scope until a main run passes. The `main` ruleset names the required checks by job name, so a renamed or added gate job also needs a ruleset change. A failing scheduled run opens a `scheduled-failure` issue for its workflow, assigned to the login in the `SCHEDULE_ALERT_ASSIGNEE` repository variable. The next passing scheduled run closes it.

The `JsonSchema` derives sit behind a `schema` feature that no dependent enables, because building them costs 14% of `cadmpeg-ir`'s generated code. `cadmpeg query schema` and the repro privacy projection read the committed output of `cadmpeg_ir::cadir_json_schema()` and `decode_sidecar_json_schema()` in [`crates/cadmpeg-ir/schema/`](crates/cadmpeg-ir/schema/). The schema gate compares those files with the derives. When you change IR or native record types, regenerate them and review the diff:

```sh
UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema-order --test committed_schema
```

The excluded fuzz crate uses Rust nightly and `cargo-fuzz`. The scheduled [fuzz smoke workflow](.github/workflows/fuzz-smoke.yml) compiles every fuzz target without running it:

```sh
cargo +nightly fuzz build --fuzz-dir crates/cadmpeg-fuzz
```

See [`crates/cadmpeg-fuzz/README.md`](crates/cadmpeg-fuzz/README.md) for seed regeneration and local fuzz-run commands.

To compare a crate's public API between Git refs, install Rust nightly and `cargo-public-api`, then run `cargo +nightly public-api -p CRATE -sss diff OLD_REF..NEW_REF` in a clean worktree. The command checks out each ref in place and restores the original ref.

---

## Preflight before a pull request

Run the preflight from the branch's worktree before you open or update a pull request:

```sh
python3 scripts/preflight.py          # static checks against the upstream branch
python3 scripts/preflight.py --list   # what each check looks for
```

Use `--no-fetch` to skip fetching the upstream branch.

---

## Windows

The Python scripts under `scripts/` run on Windows with these steps:

- Run them with `py -3` (for example `py -3 scripts/preflight.py`) or the full path of a real Python. A bare `python3` can resolve to the Microsoft Store stub, which prints "Python was not found" and fails. `scripts/preflight.py` starts its child checks with the interpreter that runs it.
- Install the GitHub CLI (`winget install GitHub.cli`) and run `gh auth login`.
- Configure your Git identity before contributing.
- Run the scripts from Git Bash, or from a shell where `git` and `cargo` are on `PATH`.
- Set `git config --global core.autocrlf false` before you clone. Or clone with `git clone -c core.autocrlf=false <url>`, which sets it in the new repository before checkout. CRLF line endings change the bytes of fixtures and generated files, and the checks that compare them byte for byte then fail.

The shell scripts (`*.sh`) need Git Bash and the commands that each script checks for at its start. For example, `scripts/restore-merge-stage.sh` needs Git and standard shell utilities on `PATH`.

---

## Pull request checklist

Follow the [pull request template](.github/PULL_REQUEST_TEMPLATE.md) and the local CI gate above. Classify decoder exactness accurately and add CAD fixtures through the corpus donation pipeline or generated fuzz seeds.

---

## Code style

Rust code follows `rustfmt` defaults and must pass `clippy` with warnings denied and `missing-docs` enabled (CI enforces this; see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)). Match the surrounding code's conventions. Decoder code must preserve byte provenance and classify exactness accurately.

---

## Conduct

Be respectful. Settle format interpretation disputes with evidence from the bytes. Harassment or abuse is not tolerated.
