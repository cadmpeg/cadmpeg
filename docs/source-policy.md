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

The checker recognizes source forms, not Rust types or data flow. It does not
prove numerical correctness, memory safety, loss fidelity, or test ownership.
Compiler checks, runtime validation, tests, and review remain necessary.
Policy changes edit the relevant rule and its tests; there is no global budget
that permits unrelated violations to replace removed ones.


## Decode allocation admission

`uncharged_decode_allocation` checks context-holding production functions in
core, IR decode paths, codec, container, assembly, Parasolid and Protein crates.
Writer and encoder paths use the sort rule's path exclusions. Nested functions
have independent contexts; closures keep the enclosing context. A borrowed
or owned context parameter, imported context alias, constructor-bound local, typed
local, context field and a DecodeContext method
establish the scope. Same-named types keep their local context fields; a
cross-file field owner must resolve uniquely.

Input-dependent `format!`, `.to_string()` and `.to_owned()` use
`ctx.format_retained(format_args!(...), operation)?` or
`ctx.copy_retained_text(text, operation)?`. The resource-only copy form
`copy_retained_text_limit` is also admitted. These operations charge before
storage creation and propagate refusal. A separate storage charge does not
admit an infallible allocating spelling. A string literal, a format with static arguments, or formatting of explicitly
typed primitives has a fixed size. Runtime format widths and precisions use
the charged format operation. An unresolved Display operand also uses that
operation.


The same rule checks `.to_vec()`, `String::from`, non-empty `vec!`, owned
`.collect()`, `.clone()` and iterator `.cloned()`, including associated trait
call spellings. Use
`copy_slice` for Copy elements, `copy_retained_strings` for strings,
`collect_vec` or `try_collect_vec` for vectors, and the charged map or set
operation for those collections. A filled vector uses `alloc_filled` with
primitive literals, explicitly typed primitives, `None`, or empty `Vec` and
`String` constructors. Other fills use `collect_indexed_vec` with explicit
Copy assignments or charged child construction. A record clone constructs its fields with charged child
copies. The rule rejects unresolved clone and collect types: Copy values use
direct copies, and a non-allocating collect uses its specific operation.
Vectors containing only literals are fixed size and excluded. Raw clones
with unresolved ownership use a direct Copy assignment or charged child copies.
Explicit `std::rc::Rc::clone` and `std::sync::Arc::clone`, including their
standard imports, only increment reference counts and allocate no child
storage. These forms are excluded. No separate charge admits raw collection
creation.


## Decode work admission

`uncharged_decode_work` has the allocation rule's context and path scope.
Input-sized `for`, `while` and `loop`, iterator consumers, searches, prefix
comparisons and decoded slice equality require propagated work admission.
Unresolved scan types use the same forms. Fixed literal ranges, arrays, byte comparisons against literals and scalar
count queries stay outside the rule. A scalar `min` or `max` with an argument is constant time.

A loop admits each iteration before its first effect on every control-flow
path. Admission can be `ctx.charge_work(..., operation)?`, the resource-only
`charge_work_limit` form, a charged `DecodeContext` operation such as
`push_vec`, `insert_btree_map` or `copy_retained_text`, or a call that passes
the caller context and propagates refusal with `?`. The checker resolves
charged context methods through the core method call graph. A charge on only
one branch does not admit a loop. A direct
`.map_err(Error::ResourceLimit)?` preserves the resource payload and is also
admitted; closures and other error constructors are not admitted. A positive literal or a checked positive increment followed directly by
`ok_or` or `ok_or_else` and `?` charges the iteration. Input-dependent body work has a separate charge. A scan can instead use a charge in the same block before it
with its exact extent, `u64_from_index(values.len())`, or the capacity of a
hash table whose buckets are scanned. A simple local extent
alias is accepted. Each charge admits one scan. Conditional, later, dropped,
zero, unrelated and reused charges do not admit it. Nested scans need their
own charge. Bodies and predicates with further input-dependent work charge
that work separately.

Use `ctx.position_by(values, fallible_predicate, operation)?` for a search
and `ctx.equal_bytes(left, right, operation)?` for decoded byte equality.
Heap-owning comparisons require charged child comparisons. Named equality
with unresolved ownership uses an explicit scalar annotation or a charged
comparison. An aggregate
algorithm bound that the lexical rule cannot prove is expressed as explicit
per-pass or per-iteration admission. The checker does not infer arbitrary
arithmetic, alias mutations or a callback's complexity.
