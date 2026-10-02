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
Its `rust-toolchain.toml` pins `nightly-2026-09-08`, with `rustc-dev`, `rust-src`,
`llvm-tools-preview` and `clippy`. The script installs missing pinned
components, builds the driver and runs one Cargo check for the selected
production libraries. Its target directory is
`target/decode-policy`. Decode package artifacts are removed before a run
so Cargo cannot omit findings for unchanged source.

The compiler resolves expressions, receiver types, associated trait calls,
record fields and closure owners. The allocation and work rules inspect
production bodies reachable from decode roots in
`cadmpeg-core`, `cadmpeg-ir`, `cadmpeg-codec-*`, `cadmpeg-container`,
`cadmpeg-asm`, `cadmpeg-parasolid` and `cadmpeg-protein`. A body without a
`DecodeContext` cannot admit input-sized storage or work. Roots are implementations
of `CodecBackend::{detect_impl, inspect_impl, decode_impl}` and
`Codec::{detect, inspect, decode, decode_with_context}`, and public functions
whose input types contain `DecodeContext`. Resolved calls, trait targets and
closures form a call graph across all checked crates. Bodies reached only
from encoding, writing, serialization or tests are outside the scope.
A writer file name does not exclude a body reached during decoding.
Binary and test bodies are excluded. Automatically derived
bodies, including serde derives and their generated helpers, are excluded.
A decode call into a derived implementation is judged at the call using the
concrete field costs. Hand-written implementations retain their own
obligations. Serialize implementation bodies are excluded; their calls retain external costs.

A resolved call to a checked body is proved at the caller. Its body owns the
admission obligation. Resolution uses the caller's compiler typing environment.
Closures have separate checked bodies. A private trait call is proved when
all reachable implementations have checked bodies. Generic helper calls use
symbolic storage and work proofs in the owning body. A propagated storage
charge of the operand count times `size_of::<T>()` admits that count of
slots for every element type. Each byte charge is consumed once; branch
joins retain only charges present on every path, and mutation invalidates
operand evidence. Typed core collection admission carries the target and
count. Raw slot growth consumes the matching admission; an aggregate count
is scaled across its bounded loop. Exact vector growth compares the charged
capacity delta with the requested capacity delta. Copy bounds prove fixed
per-element work without a concrete element size. Concrete instantiations judge costs that run through
uncharged type-parameter traits, including Clone, comparison, hash and
conversion. Their defects are reported at the instantiating call, with the
concrete type. Checked dependency generics use compiler MIR to resolve their
operation calls. Local and imported bodies use the same exclusions. The driver encodes MIR in check metadata for this resolution. Trait-object and function-pointer calls require proof.

`src/external.rs` in the checker defines the external operation summaries.
Each summary states allocation behavior and fixed work, receiver work,
argument or key work, iterator work, or comparison work. The allocation and
work rules evaluate these extents against operand types and prior admission.
Summaries distinguish fixed moves and constructors from input-sized
operations with the same name. An external operation missing from this table
uses the third rule, including calls with scalar operands or no operands.
Run `python3 scripts/check-decode-policy.py --list-externals --output FILE`
to list every distinct resolved external operation reached by checked
production bodies. The TSV columns are `external_operation`, resolved path,
allocation behavior and work extent. `Argument(N)` names a zero-based
operand, including the receiver at zero. `Input(N)` denotes result or
internal allocation bounded by that operand. Type-dependent Clone,
conversion and result summaries use the concrete type. `None` and `Fixed`
have no input-sized allocation and no input-sized work. A missing entry
prints `MISSING` and fails inventory mode. Compiler errors also fail it. `--external-output FILE` also saves the
inventory during a findings run.

### Allocation

`uncharged_decode_allocation` reports an operation that allocates owned
storage whose size depends on input without a core charged operation.
Ownership follows `String`, `Vec`, boxed slices, maps, sets and records that
own such values. Borrowed values and types with no heap storage do not
allocate when copied. The rule checks standard allocating constructors,
`format!`, `to_string`, `to_owned`, `to_vec`, heap `collect`, `vec!`, `From`
and `Into`, derived or standard heap `Clone`, and collection growth.
Literal text, static string constants, and values constructed only from
fixed operands have fixed extents. These facts cross local and imported
generic calls, including nested error constructors. Mutation invalidates
them. Runtime repetition counts remain variable.
`Vec::new`, `String::new` and empty collection constructors allocate no
storage. Moving an owned value does not allocate. A fresh owned vector iterator
collected into the same vector type reuses its buffer without a scan.
Consumed or adapted owning iterators require proof of storage reuse. A custom
`Clone` is checked in its body, including temporary storage when its result
borrows data.
Owning a heap field alone does not prove that it allocates. Derived clones
follow each field's concrete clone implementation. Zero-sized vector
elements require no backing allocation. A borrowed `Cow` conversion does
not allocate.

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
A `take` bound does not make an input-sized source fixed. A slice iterator's
`count` and integer or character range `count` use metadata or bounded
arithmetic. Filling a unit-element vector sets its length without a scan.
Copies of zero-sized Copy elements transfer no bytes. Unknown iterator
implementations and element layouts require proof before these exceptions
apply.

Use `ctx.charge_work(extent, operation)?` or `charge_work_limit` before the
operation. The charge must resolve to the core operation; a same-named
wrapper does not establish admission. A simple extent alias can carry a length.
Checked addition carries summed extents across sequential loops. Checked multiplication carries a fixed count
across nested loops. Exact checked conversions preserve the extent.
`windows`, `chunks`, `chunks_exact`, `skip`, `take`, `step_by`, `filter`,
`map`, `enumerate`, range sub-slices, `split_at` halves and range `get` preserve
an operand bound. Either charged side can bound a `zip`. A parent bound does
not bound a variable-size child. Each portion of a charge is consumed once.
A conditional, later, dropped, reused or unrelated charge does
not admit it. Mutation or mutable access invalidates extent evidence.
Charges outside an input-sized loop or deferred closure do not admit child
scans. A checked product admits the corresponding fixed-count loop.
A loop can instead admit every iteration path with a propagated context
operation before work. Filtering and skipping can inspect input before a
yielded iteration; a charge in that iteration does not admit those visits.

Use `ctx.position_by` for fallible search and `ctx.equal_bytes` for byte
comparison. A checked context operation owns its work admission. Its call
site carries no duplicate body finding. An unavailable or unresolved implementation uses
the third rule. Arguments and callbacks remain checked. Custom comparison implementations are inspected separately from their owning
types.

### Undecided operations

`unproven_decode_charge` reports operations for which type resolution does
not establish allocator reachability, extent or charge coverage.
Unenumerated generic instantiations, trait objects, function pointers,
external operations missing a summary, unresolved callbacks and unsupported
charge arithmetic use this rule. A nonstandard formatter whose output
extent is not proved also uses this rule; an owned field does not prove
that its formatter reads that field. These findings fail the gate.
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
