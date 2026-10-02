# Source policy

Run `python3 scripts/check-source-policy.py` to check the current source tree.
The check needs no Git history, baseline, ledger, or update step. Exit status
is 0 for clean source and 1 for violations. Text and `--json` output identify
each violation by rule, file, line, and explanation.
Use repeatable `--crate NAME` arguments to restrict reported findings to named crates.

## Rules

- Standard-width file reads use bounded `View` readers. Direct endian
  conversions require an explicit local exception.
- Calls whose return type contains `EvaluationFailure` keep resource refusals.
  Success-only patterns, wildcard error arms, error-dropping result methods,
  ignored `map_err` arguments, and error-dropping iterator adapters fail.
  Use `finite_or_refusal`, `non_finite`, `?`, or a propagating `ResourceLimit`
  arm. The rule discovers evaluator names from production return signatures,
  resolves function paths and imports, and tracks bound results. Common method
  names require a receiver type or constructor that identifies the evaluator.
- Slice sort calls in functions with a borrowed `DecodeContext` use
  `ctx.stable_sort_by` or `ctx.sort_unstable_by`. This includes typed context
  locals and context fields accessed through `self`. The two core sort
  implementations and test code are exempt. Functions without a context stay
  outside this rule.
- Loss notes use the owning loss code's `note` method.
- Formatted malformed errors use structured codec errors.
- Tolerances from `1e-6` through `1e-12` use named constants or statics.
- Vector repeats use literal sizes, admitted collection lengths, or checked
  allocation. This check recognizes syntax; it does not prove count safety.
- A production `let _ =` states why the value it drops has no reader. Sites
  with no reason fail.
- Unit tests belong to their production owner. Crate-root `src/tests.rs` and
  test-only `#[path]` module includes are prohibited.
- Test files and inline test modules have a 2,000-line limit. Golden test files
  are excluded. Production files have a 10,000-line limit after removing
  `cfg(test)` items. These are maintenance limits, not correctness proofs.
- A module-level `fn`, `const` or `static` claims no more reach than its module
  can grant. A module the declaration chain caps below the crate root cannot be
  named from outside that cap, so `pub(crate)` on such an item spells reach the
  module already denies. Associated items, struct fields, enum variants and
  types stay outside the rule: the compiler can require the wider marker for
  them. A module named by a non-private `use` keeps the reach the re-export
  grants.
- Every member of a serde wire mirror in `cadmpeg-ir`, `cadmpeg-core` and
  `cadmpeg-asm` carries a doc comment. These crates publish JSON Schema through
  their `schema` features. The mirror is the type a serde conversion container
  attribute names with `try_from`, `from` or `into`, and its members are its
  fields, an enum's variants and the fields of a struct-shaped variant. The
  published JSON schema reads each member's doc as that property's
  `description`, so a member with no doc leaves the schema silent about the
  value the wire carries. A codec crate's own records stay outside the rule:
  they generate no schema and state their shape through `NativeRecord`. An
  unqualified target resolves in the file that names it, through that file's
  `use` imports, and then to a unique crate-wide declaration. An ambiguous
  unqualified target is unresolved. A qualified target resolves along the
  `crate`, `self`, `super` or module path that it names. A path whose leading
  segment is not a module of the crate names an external crate and is not
  resolved. A member the mirror carries with
  `#[serde(flatten)]` publishes no property of its own; the properties are the
  members of the flattened type, so that type is a mirror as well and the rule
  repeats through a chain of flattened types. The flattened type's name is
  resolved the same way. A type the flattened type reaches through anything
  other than a further `#[serde(flatten)]` is not resolved.
- Every test a `scripts/test_*.py` file declares is collected. A test case class
  or a free `test_` function declared at or after the file's
  `if __name__ == "__main__":` block fails: discovery imports the module and
  never runs that block.

## Checked arithmetic

Production `saturating_*` calls fail with `saturating_arithmetic`. Use checked
arithmetic. The overflow branch propagates the caller context's resource refusal
in decode code or a typed error elsewhere. Constant expressions must preserve
the exact value or reject the invalid constant.

## Lint suppressions

`lint_suppression` rejects production `#[allow(...)]` and `#[expect(...)]`,
including inner attributes and conditional suppressions. Fix the code covered
by the lint. Test modules and test files retain their suppressions. The one
exception is the module-level expectation in `cadmpeg-core/src/convert.rs` for
`as_conversions`, `cast_possible_truncation`, `cast_precision_loss` and
`cast_sign_loss`. It has a nonempty reason and appears once. Function-level
expectations, nested module expectations and additional lints are rejected.

## Integer limits

`integer_clamp` rejects integer `MAX` or `MIN` defaults in `unwrap_or`,
`unwrap_or_else`, `map_or` and `map_or_else`. Qualified primitive paths,
parenthesized bounds, closure parameter patterns, closure blocks and explicit
closure return types are included. Use an exact core conversion or `From` when
the conversion cannot fail on supported targets; use `TryFrom` with a refusal
branch when it can fail. Missing range endpoints and sort keys use `Option`.
Limits in successful mapping arms and arguments to refusal functions are not
default bounds.

## Wrapping exceptions

Production `wrapping_*` calls fail with `wrapping_arithmetic` unless the file
format defines modular arithmetic and a standalone `// wrapping-exception:
<reason>` comment immediately precedes exactly one call. The nonempty reason
states the modular operation. Index and length arithmetic use checked operations
and an explicit overflow branch. Stale markers and markers before multiple calls
fail with `wrapping_exception`. Test-only markers and calls are excluded. Comments
inside literals and trailing comments grant no exception.

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

## Discarded values

`let _ = ...` and `let _: T = ...` drop a value the code has already computed.
Most such sites are a refusal to thread through `?` or a binding to delete. A
site that survives states its reason in a standalone line comment immediately
above it, which admits exactly one discard on the next line:

```rust
// discarded-value: the overflow test is the whole effect; ? states the refusal
let _ = self.position().checked_add(len).ok_or_else(|| error())?;
```

The reason is free prose and must not be empty. Stale reasons — a comment with
no discard on the next line — fail, and comments inside Rust strings grant
nothing. Fuzz entry-point files are listed in the checker with the reason they
are outside the rule: such a wrapper's whole contract is to run a parser over
arbitrary bytes and drop the answer.

## Scope and limits

Source-pattern rules inspect production Rust under `crates/**/src`. They exclude
test, test-support, golden, integration, and bench paths and filenames containing
`test`. Comments, literals, and `cfg(test)` items are masked before matching.
Placement rules inspect `crates/**/*.rs`, following test-only module ancestry.
The test-collection rule reads `scripts/test_*.py` as Python syntax.
Both scans recognize `cfg(test)` and flat `cfg(all(..., test, ...))` gates.
Other conditions remain production, including `cfg(not(test))` and
`cfg(any(feature = "examples", test))`. Comments and literals are masked
before test-item boundaries and vector repeats are scanned.

The checker recognizes source forms, not Rust types or data flow. It does not
prove numerical correctness, memory safety, loss fidelity, or test ownership.
Compiler checks, runtime validation, tests, and review remain necessary.
Policy changes edit the relevant rule and its tests; there is no global budget
that permits unrelated violations to replace removed ones.
