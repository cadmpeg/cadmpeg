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

PRs that only touch tooling, the IR, exporters, tests, or docs do not need the provenance declaration (but still need the DCO sign-off).

---

## Licensing of contributions

By contributing, you license code under **Apache-2.0** and documentation/spec content under **CC-BY-4.0**, matching the split described in the [README](README.md). No separate CLA is required. The DCO sign-off asserts your right to contribute.

---

## Ways to contribute

### Contribute a codec (turn a spec into a decoder)

A codec takes native bytes and produces the cadmpeg IR.

1. **Start from a spec.** Pick a format in [`docs/formats/`](docs/formats/). If the spec has open gates, implement only the byte-proven subset.
2. **Implement the codec interface.** Codecs plug in through a common trait with three responsibilities: _detect_ (is this my format?), _inspect_ (report containers/streams/structure without full decode), and _decode_ (produce IR). See [docs/architecture.md](docs/architecture.md) for the codec plugin model and the exact trait; it lives in the Rust workspace.
3. **Respect the two tiers.** Values you read directly from bytes are strict. Label values you infer or repair as inferred in the IR; never present an inferred value as if it were byte-derived.
4. **Account for loss.** If your decoder skips or approximates something, surface it; do not silently drop bytes.
5. **Test against the corpus.** Add openly-licensed test files through the [corpus donation pipeline](corpus/README.md) rather than committing binaries directly, and add golden/round-trip tests where you can.
6. **Include the provenance declaration** in your PR.

### Contribute format research (extend a spec)

Format research starts with a hypothesis about an unknown file region. Test that hypothesis against files you authored with controlled variations. Record what the bytes prove and what they disprove.

1. Work in the relevant [`docs/formats/`](docs/formats/) spec.
2. Document findings with evidence: byte offsets, hex, and the reasoning that ties a region to a meaning. Record falsified hypotheses to avoid repeated analysis.
3. Keep the "open gates" / remaining-unknowns section of the spec current.
4. Include the provenance declaration; format research is derived-knowledge work and the clean-room rules apply in full.

### Everything else

IR schema tooling, validators, exporters, corpus tooling, hex-annotation tooling, CLI ergonomics, and docs fixes need only the DCO sign-off. See [docs/roadmap.md](docs/roadmap.md) for starter issues.

---

## Pull request checklist

- [ ] Commits are signed off (`git commit -s`).
- [ ] For decoder/spec PRs: the provenance declaration is in the description.
- [ ] `cargo fmt --all` is clean and `cargo clippy --workspace` has no warnings.
- [ ] `cargo test --workspace` passes.
- [ ] No CAD binaries committed outside the corpus donation pipeline or the generated fuzz seeds (see [`seeds/README.md`](seeds/README.md)).
- [ ] Inferred/repaired values are labeled as such in the IR (decoder PRs).

---

## Code style

Rust code follows `rustfmt` defaults and must pass `clippy` with warnings denied (CI enforces this; see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)). Match the surrounding code's conventions. Decoder code should preserve byte provenance and make inferred values visible.

---

## Conduct

Be respectful. Settle format interpretation disputes with evidence from the bytes. Harassment or abuse is not tolerated.
