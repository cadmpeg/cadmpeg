# Source policy

Run `python3 scripts/check-source-policy.py` to check the current source tree.
The check needs no Git history, baseline, ledger, or update step. Exit status
is 0 for clean source and 1 for violations. Text and `--json` output identify
each violation by rule, file, line, and explanation.

## Rules

- Standard-width file reads use bounded `View` readers. Direct endian
  conversions require an explicit local exception.
- Loss notes use the owning loss code's `note` method.
- Formatted malformed errors use structured codec errors.
- Tolerances from `1e-6` through `1e-12` use named constants or statics.
- Vector repeats use literal sizes, admitted collection lengths, or checked
  allocation. This check recognizes syntax; it does not prove count safety.
- Unit tests belong to their production owner. Crate-root `src/tests.rs` and
  test-only `#[path]` module includes are prohibited.
- Test files and inline test modules have a 2,000-line limit. Golden test files
  are excluded. Production files have a 10,000-line limit after removing
  `cfg(test)` items. These are maintenance limits, not correctness proofs.

## Endian exceptions

A standalone line comment immediately before a conversion admits exactly one
call on the next line:

```rust
// endian-exception: reconstructed-scalar
f64::from_be_bytes(reconstructed)
```

The two reasons are `reconstructed-scalar` for reconstructed numeric byte
representations and `packed-color-order` for in-memory color sort keys. Neither
permits an ordinary standard-width file read. Reviewers must verify that the
reason matches the operation. Unknown or stale exceptions fail. Comments inside
Rust strings do not grant exceptions.

## Scope and limits

Source-pattern rules inspect production Rust under `crates/**/src`. They exclude
test, test-support, golden, integration, and bench paths and filenames containing
`test`. Comments, literals, and `cfg(test)` items are masked before matching.
Placement rules inspect `crates/**/*.rs`, following test-only module ancestry.

The checker recognizes source forms, not Rust types or data flow. It does not
prove numerical correctness, memory safety, loss fidelity, or test ownership.
Compiler checks, runtime validation, tests, and review remain necessary.
Policy changes edit the relevant rule and its tests; there is no global budget
that permits unrelated violations to replace removed ones.
