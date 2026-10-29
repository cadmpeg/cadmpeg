# Contributing to cadmpeg

Read [LEGAL.md](LEGAL.md) before contributing. It defines where format knowledge may and may not come from, and it is binding.

---

## Two hard requirements

### 1. DCO sign-off

cadmpeg uses the [Developer Certificate of Origin](https://developercertificate.org/). Every pull request must be signed off. The sign-off certifies that you have the right to submit the contribution under the project's licenses.

Pull requests are squash-merged, so the sign-off covers the pull request as a whole, not each commit. Sign off by ticking the `Signed off` box in the pull request template. The ticked box is the sign-off. It needs no `Signed-off-by` line beside it, and the commits need no `git commit -s`.

CI also accepts a `Signed-off-by: Your Name <your@email>` line in the description, or a sign-off on every commit. Neither is required when the box is ticked.

Use a consistent contributor identity and an email address where you can be reached. Do not forge authorship or sign off for someone else.

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

1. **Start from a spec.** Pick a format in [`docs/formats/`](docs/formats/) and review its companion `*-open-items.md` file. Implement only settled byte semantics.
2. **Implement the codec interface.** Codecs implement the [`Codec` trait in `crates/cadmpeg-ir/src/codec.rs`](crates/cadmpeg-ir/src/codec.rs), which defines `id`, `detect`, `inspect`, and `decode`. See [docs/architecture.md](docs/architecture.md) for the plugin model.
3. **Classify exactness correctly.** `ByteExact` means read from the source stream without transformation beyond documented unit conversion. `Derived` means computed deterministically from byte-exact inputs. `Inferred` means supplied from context or convention rather than an explicit source field. `Unknown` means origin or trustworthiness could not be established.
4. **Account for loss.** If your decoder skips or approximates something, surface it; do not silently drop bytes.
5. **Add authorized fixtures.** Add public CAD fixtures through the [corpus donation pipeline](corpus/README.md), not as untracked test binaries. Add focused decode, validation, and round-trip tests where applicable.
6. **Include the provenance declaration** in your PR.

### Contribute format research (extend a spec)

Format research starts with a hypothesis about an unknown file region. Test that hypothesis against files you authored with controlled variations. Record what the bytes prove and what they disprove.

1. Work in the relevant [`docs/formats/`](docs/formats/) spec.
2. Document findings with evidence: byte offsets, hex, and the reasoning that ties a region to a meaning. Record falsified hypotheses to avoid repeated analysis.
3. Keep the companion `*-open-items.md` file current. Specs contain settled byte semantics and invariants; unresolved questions belong in the open-items file. An item uses only Question, Known, Need, Conflict, and Note. When the answer is settled, delete the item in the same change that writes it into the specification. Do not keep a Resolved part.
4. Include the provenance declaration; format research is derived-knowledge work and the clean-room rules apply in full.

### Everything else

IR schema tooling, validators, exporters, corpus tooling, hex-annotation tooling, CLI ergonomics, and docs fixes need only the DCO sign-off. See the [roadmap contributor entry points](docs/roadmap.md#contributor-entry-points) for bounded ways to contribute.

---

## Local CI gate

Run the stable CI gate from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo build --workspace
cargo test-fast
```

Lint levels live in `[workspace.lints]` in the root `Cargo.toml`, so `cargo clippy` needs no trailing lint flags. Do not add them back: trailing lint flags are part of the compilation fingerprint, so a flagged run and a bare run evict each other's cached artifacts, and an editor running bare `cargo check` or `cargo clippy` then re-checks the workspace on every alternation.

Manifest levels apply to every cargo command, so a warning fails `cargo build` and `cargo test`, not only `cargo clippy`. Jobs on an unpinned toolchain set `RUSTFLAGS: -A warnings`, because lint sets differ between releases; the MSRV job is the only one.

CI builds test binaries without debug information. A failure there reports a backtrace without `file:line`; run the failing test locally to get the line numbers back.

PR CI tests only the packages a change reaches ([`scripts/ci_scope.py`](scripts/ci_scope.py)). A draft PR runs the static checks and clippy only. macOS and Windows run in the nightly full gate, and on a `main` or merge-queue commit that selects the full gate, which includes every release bump. To run them on a branch, dispatch CI on it: `gh workflow run ci.yml --ref BRANCH`. CodeQL analyzes Rust only in its nightly run.

The `JsonSchema` derives sit behind a `schema` feature that is off by default, because only `cadmpeg_ir::cadir_json_schema()` consumes them and building them costs 26% of `cadmpeg-ir`'s compile time. Run the schema side when you change IR or native record types:

```sh
cargo test -p cadmpeg-ir --features schema --lib
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
python3 scripts/preflight.py --full   # also the Cargo checks for the crates the change reaches
python3 scripts/preflight.py --list   # what each check looks for
```

The preflight finds the remote that points at `cadmpeg/cadmpeg`, or `origin`. It fetches that remote's default branch and checks the working tree against the merge base.

Each check prints `PASS`, `FAIL`, `WARN`, or `SKIP`, with the fix. Any `FAIL` exits 1. A `FAIL` is something review or CI rejects. Examples are an `IR_VERSION` change with stale fixtures, an open-item ID that upstream already uses, and a branch that conflicts with upstream. A `WARN` needs your judgement.

Without `--full`, nothing compiles. `--run-layout` regenerates changed layout tables; a layout page or `layout.rs` that changed without its table fails unless regeneration reproduces it byte for byte. `--rev REV` checks a commit instead of the working tree.

The `open-prs` check lists the upstream open pull requests through the GitHub CLI, so it needs network access and an authenticated `gh` (`gh auth login`). Without them it is skipped. `--offline` skips it and the fetch.

`spec-wording` warns on added format Markdown lines containing “not yet”, “do not yet”, “for now”, “currently”, or “TODO”, including open-items documents.

The checks cover recurring review points, not all of them. A check is a `scripts/preflight_<name>.py` module with a `scripts/test_preflight_<name>.py` test.

---

## Pull request checklist

- [ ] The `Signed off` box in the pull request description is ticked.
- [ ] For decoder/spec PRs: the provenance declaration is in the description.
- [ ] `python3 scripts/preflight.py` reports no `FAIL`.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --workspace --all-targets` passes.
- [ ] `cargo build --workspace` passes.
- [ ] `cargo test-fast` and `cargo test --workspace --doc` pass.
- [ ] `cargo test -p cadmpeg-ir --features schema --lib` passes (IR or native record type changes).
- [ ] No CAD binaries committed outside the corpus donation pipeline or the generated fuzz seeds (see [`crates/cadmpeg-fuzz/README.md`](crates/cadmpeg-fuzz/README.md)).
- [ ] IR exactness is classified as `ByteExact`, `Derived`, `Inferred`, or `Unknown` accurately (decoder PRs).

---

## Code style

Rust code follows `rustfmt` defaults and must pass `clippy` with warnings denied and `missing-docs` enabled (CI enforces this; see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)). Match the surrounding code's conventions. Decoder code must preserve byte provenance and classify exactness accurately.

---

## Conduct

Be respectful. Settle format interpretation disputes with evidence from the bytes. Harassment or abuse is not tolerated.
