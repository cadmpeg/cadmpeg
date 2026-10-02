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

The checker recognizes source forms, type annotations and direct local extents.
It does not perform Rust type checking or general data-flow analysis. It does not
prove numerical correctness, memory safety, loss fidelity, or test ownership.
Compiler checks, runtime validation, tests, and review remain necessary.
Policy changes edit the relevant rule and its tests; there is no global budget
that permits unrelated violations to replace removed ones.


## Typed decode admission

Run `python3 scripts/check-decode-policy.py`. The pre-commit, CI and release
gates run this script. Repeat `--crate NAME` to select decode packages; their
workspace dependencies are also checked. `--output FILE` writes sorted TSV
findings. The four columns are rule, path, line and message. Exit status is 1
on any finding. Compiler failures retain their nonzero exit status.

The tool is `crates/cadmpeg-decode-policy`, outside the default workspace.
Its `rust-toolchain.toml` pins `nightly-2026-09-08`, with `rustc-dev`, `rust-src`
and `llvm-tools-preview`. The script installs missing pinned components, builds the driver and runs one Cargo
check for the selected production libraries. Its target directory is
`target/decode-policy`. Decode package artifacts are removed before a run
so Cargo cannot omit findings for unchanged source.

The compiler resolves expressions, receiver types, associated trait calls,
record fields and closure owners. Both rules inspect production functions
that hold the caller's `DecodeContext` in `cadmpeg-core`, `cadmpeg-ir`,
`cadmpeg-codec-*`, `cadmpeg-container`, `cadmpeg-asm`, `cadmpeg-parasolid`
and `cadmpeg-protein`. Writer, encoder, binary and test paths are excluded.
Nested functions have independent scopes. Closures retain their enclosing
function's scope. An owned or borrowed parameter, a local context, a context
field or a context method establishes the scope.

### Allocation

`uncharged_decode_allocation` reports an operation that allocates owned
storage whose size depends on input without a core charged operation.
Ownership follows `String`, `Vec`, boxed slices, maps, sets and records that
own such values. Borrowed values and types with no heap storage do not
allocate when copied. The rule checks standard allocating constructors,
`format!`, `to_string`, `to_owned`, `to_vec`, heap `collect`, `vec!`, `From`
and `Into`, derived or standard heap `Clone`, and collection growth.
`Vec::new`, `String::new` and empty collection constructors allocate no
storage. Moving an owned value does not allocate. A custom `Clone` is checked
in its body; owning a heap field alone does not prove that it allocates.

Use `ctx.copy_retained_text` or `copy_retained_text_limit` for text copies,
`ctx.format_retained(format_args!(...), operation)?` for variable text,
`ctx.copy_slice` for Copy elements, `ctx.copy_retained_strings` for string
children, and `ctx.collect_vec` or `try_collect_vec` for vectors. Use the
matching core map or set operation for those collections. Use
`ctx.alloc_filled` with Copy or empty values. Heap child clones use
`ctx.collect_indexed_vec` and charged child construction. A charged outer
collection does not admit uncharged child clones.

Literal text, numeric formatting, fixed-size Copy enums, fixed arrays and
constant-bounded collection construction have an input-independent size.
`Rc::clone` and `Arc::clone` allocate no child storage. A runtime format width
or precision requires the charged format operation. A separate storage
charge does not admit an infallible raw allocation in a caller.

### Work

`uncharged_decode_work` reports input-sized loops, iterator consumption,
comparisons, searches, hashes and copies without work admission. Slices,
strings, vectors, maps, sets and `View` have variable extents. Scalars,
fixed-size Copy values, arrays and constant-bounded ranges have fixed
extents. Fixed array slots do not admit variable-size child comparisons.
A `take` bound does not make an input-sized source fixed.

Use `ctx.charge_work(extent, operation)?` or `charge_work_limit` before the
operation. A simple extent alias can carry a length. Additive lengths and
propagated checked sums can carry comparison bounds. A credit admits one
operation. A conditional, later, dropped, reused or unrelated charge does
not admit it. Mutation or mutable access invalidates extent evidence.
Charges outside a loop or deferred closure do not admit its child scans.
A loop can instead admit every iteration path with a propagated context
operation before work. Filtering and skipping can inspect input before a
yielded iteration; a charge in that iteration does not admit those visits.

Use `ctx.position_by` for fallible search and `ctx.equal_bytes` for byte
comparison. A callee that takes the context owns its work admission. Its
arguments and callbacks remain checked. Custom comparison implementations
are inspected separately from their owning types.

### Undecided operations

`unproven_decode_charge` reports operations for which type resolution does
not establish allocator reachability, extent or charge coverage. Generic
values, trait objects, opaque callees, unresolved callbacks and unsupported
charge arithmetic use this rule. These are findings and fail the gate.
They are not reported as known allocation or work defects. Use a concrete
type, a core charged operation, or explicit admission with a direct extent
inside the operation. A length charge alone does not establish child-byte,
hash-capacity or sorting work coverage. A storage charge does not admit
work. The checker does not silently accept an undecided
operation.

The fixture tests are in `crates/cadmpeg-decode-policy/fixtures` and are run
by the compiler integration suite:

```
cargo +nightly-2026-09-08 test -q --manifest-path crates/cadmpeg-decode-policy/Cargo.toml --lib
```
