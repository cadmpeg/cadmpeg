# Contributing to cadmpeg

Read [LEGAL.md](LEGAL.md) before contributing. It defines where format knowledge may and may not come from, and it is binding.

---

## Two hard requirements

### 1. DCO sign-off on every commit

cadmpeg uses the [Developer Certificate of Origin](https://developercertificate.org/). Every commit must be signed off. The sign-off certifies that you have the right to submit the contribution under the project's licenses:

```sh
git commit -s -m "Your message"
```

This appends a `Signed-off-by: Your Name <your@email>` line. Commits without sign-off cannot be merged. Use a consistent contributor identity and an email address where you can be reached. Do not forge authorship or sign off for someone else.

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
3. Keep the companion `*-open-items.md` file current. Specs contain settled byte semantics and invariants; unresolved questions belong in the open-items file.
4. Include the provenance declaration; format research is derived-knowledge work and the clean-room rules apply in full.

### Everything else

IR schema tooling, validators, exporters, corpus tooling, hex-annotation tooling, CLI ergonomics, and docs fixes need only the DCO sign-off. See the [roadmap contributor entry points](docs/roadmap.md#contributor-entry-points) for bounded ways to contribute.

---

## Local CI gate

Run the stable CI gate from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace -- -D warnings -W missing-docs
cargo build --workspace
cargo test-fast
```

The excluded fuzz crate uses Rust nightly and `cargo-fuzz`. The scheduled [fuzz smoke workflow](.github/workflows/fuzz-smoke.yml) compiles every fuzz target without running it:

```sh
cargo +nightly fuzz build --fuzz-dir crates/cadmpeg-fuzz
```

See [`seeds/README.md`](seeds/README.md) for seed regeneration and local fuzz-run commands.

---

## Pull request checklist

- [ ] Commits are signed off (`git commit -s`).
- [ ] For decoder/spec PRs: the provenance declaration is in the description.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --workspace -- -D warnings -W missing-docs` passes.
- [ ] `cargo build --workspace` passes.
- [ ] `cargo test-fast` and `cargo test --workspace --doc` pass.
- [ ] No CAD binaries committed outside the corpus donation pipeline or the generated fuzz seeds (see [`seeds/README.md`](seeds/README.md)).
- [ ] IR exactness is classified as `ByteExact`, `Derived`, `Inferred`, or `Unknown` accurately (decoder PRs).

---

## Code style

Rust code follows `rustfmt` defaults and must pass `clippy` with warnings denied and `missing-docs` enabled (CI enforces this; see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)). Match the surrounding code's conventions. Decoder code must preserve byte provenance and classify exactness accurately.

---

## Conduct

Be respectful. Settle format interpretation disputes with evidence from the bytes. Harassment or abuse is not tolerated.
