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
- Test declarations in `scripts/test_*.py` precede the file's
  `if __name__ == "__main__":` block. A direct run calls `unittest.main()` in
  that block before any later definition exists, and discovery never runs the
  block, so a test declared after it is missed by direct runs and one declared
  inside it is missed by discovery.

## Numeric casts

Workspace Clippy lints deny `as_conversions`, `cast_possible_truncation`,
`cast_possible_wrap`, `cast_sign_loss`, `cast_precision_loss` and `cast_lossless`.
Numeric casts occur only in `cadmpeg-core/src/convert.rs`, under its one module
expectation. Each conversion checks exactness or the finite target range. All
other conversions use the core functions, `From`, or `TryFrom` with an explicit
refusal branch.

## Checked arithmetic

Production `saturating_*` calls fail with `saturating_arithmetic`. Use checked
arithmetic. The overflow branch propagates the caller context's resource refusal
in decode code or a typed error elsewhere. Constant expressions must preserve
the exact value or reject the invalid constant.

## Decode context parameters

`optional_decode_context` rejects production function parameters with type
`Option<&DecodeContext<...>>`. Qualified paths, explicit borrow lifetimes,
mutable borrows, trait methods and function-pointer parameters are included.
The decode path passes its caller context. Context-free reconstruction and
writers take no context. Fields, local bindings and returned values are not
function parameters and stay outside this rule. The checker recognizes the
named type in source and does not resolve type aliases.

## Lint suppressions

`lint_suppression` rejects production `#[allow(...)]` and `#[expect(...)]`,
including inner attributes and conditional suppressions active in production.
Suppressions under `cfg_attr(test, ...)` or a flat `all(..., test, ...)`
predicate apply only to tests and are exempt. Fix the code covered
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
components and builds the driver. A graph pass checks all production
libraries; a findings pass checks the selected libraries against the joined
scope. Its target directory is
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
whose input types contain `DecodeContext`. Resolved calls, function addresses,
static and const initializers, trait targets
and closures form a call graph across all checked crates. Initializer evaluation
has separate compile-time reachability. Its calls do not execute during decode.
Function addresses produced by a reachable initializer retain runtime reachability.
A direct runtime call of a const function retains its runtime body obligations.
A reachable function address reaches its body even without a direct call.
Concrete object coercions through references, Box, Arc and nested pointer types
reach only methods called through a reachable trait object. Calls through a
subtrait select methods in its supertrait vtable. Uncalled methods, including
Self: Sized methods, do not gain reachability from a coercion. Default methods
remain checked when called. Concrete generic instances resolve parameter
calls through local and dependency MIR before body selection. Their targets are
attached to the originating caller. Unconstrained generic decode roots carry
symbolic type arguments through helper chains and reach each possible checked
trait implementation. Concrete encoder instances do not activate those symbolic
edges. Generic pointer calls in private helpers and closures use concrete
MIR signatures; their symbolic signatures activate only under symbolic
reachability. An unresolved indirect call retains
`unproven_decode_charge`. A function-pointer call reaches address-taken
functions and noncapturing closures with the same signature after lifetime erasure, including
argument types, result type, safety and ABI. Type and const parameters use
consistent substitutions across the signature. Function addresses retain their
resolved implementation and their coerced pointer signatures. A virtual method
address retains a method-specific dispatch node. Recursive object coercions
reuse the same concrete method instance. A trait-object call reaches only
implementations of its called trait method with compatible trait arguments,
method arguments and result types. Associated types normalize in the caller
and implementation typing environments. Generic candidate bodies retain their
symbolic helper calls. Unrelated uncertainty does not
activate function addresses or object methods. A compatible fallback can reach
a target whose address was created by an encoder. Definition hashes join
local and dependency nodes. The driver collects the complete graph before
it selects any bodies, including when `--crate` limits findings. Bodies reached only
from encoding, writing, serialization or tests are outside the scope.
Run `python3 scripts/check-decode-policy.py --list-unreachable --output FILE`
to list excluded bodies as TSV: record kind, path, line, definition name and
reason. `--unreachable-output FILE` saves the same listing during a findings run.
The listing includes hand-written serialization bodies and compiled test bodies;
a cfg-disabled body has no compiler definition. Reasons are encoder-only,
writer-only, serialization-only, test-only or no path from a decode entry point.
The first two classify excluded body names or source modules; a source-module
name cannot exclude a reachable production body.
A writer file name does not exclude a body reached during decoding.
`--explain-body NAME` prints one shortest path from a decode root to a named
body. `--explain-body PATH:LINE` selects the innermost body at that source line.
Each edge states direct call, function address, trait-object call, generic
instantiation, constant evaluation or unresolved-indirect candidate. An excluded target prints
unreachable. `--graph-output FILE` saves the graph. `--graph-input FILE` uses
that saved graph for path queries without compiling; the graph describes the
source and compiler configuration of the run that created it.
Binary and test bodies are excluded. Automatically derived
bodies, including serde derives and their generated helpers, are excluded.
A decode call into a derived implementation is judged at the call using the
concrete field costs. Serialize and Deserialize implementation bodies are
excluded. Deserialization calls retain their caller admission obligation.
Other hand-written implementations retain their body obligations.

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
is scaled across its bounded loop. Unused slots remain with the same receipt
after admitted growth and a bounded loop. Excess growth requires another
admission. Exact vector growth compares the charged
capacity delta with the requested capacity delta. Concrete slot sizes reduce
to byte coefficients. A propagated byte charge admits an equal or dominated
raw exact reserve once. The admitted reserve carries backing slots to its
subsequent insertion or copy; owned child construction remains separate. Copy bounds prove fixed
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
`ctx.alloc_filled` for Copy values. Empty owned values and heap child copies use
`ctx.collect_indexed_vec` and charged child construction. A charged outer
collection does not admit uncharged child clones.

Literal text, numeric formatting, fixed-size Copy enums, fixed arrays and
constant-bounded collection construction have an input-independent size.
`Rc::clone` and `Arc::clone` allocate no child storage. A runtime format width
or precision requires the charged format operation. A separate storage
charge admits an owned text conversion only when it matches the operand byte
length and precedes the conversion on every path. The receipt admits that
conversion once, including its copy. Resolved generic forwarding preserves
the receipt when the operand reaches exactly one conversion without mutation.
Unrelated, discarded, conditional and reused receipts do not admit a conversion.
Conversion summaries reject recursion, control-flow cycles and mutable operand
aliases. Repeated raw operations require the full loop extent or a charge
inside each iteration. A backing receipt survives an exact reserve only on
a path that propagates allocation failure.
Other infallible raw allocations require their core charged operation.

### Work

`uncharged_decode_work` reports input-sized loops, iterator consumption,
comparisons, searches, hashes and copies without work admission. Slices,
strings, vectors, maps, sets and `View` have variable extents. Scalars,
fixed-size Copy values, arrays and constant-bounded ranges have fixed
extents. Constant-width indexing and range `get` preserve fixed extent,
including `at..at + N`. `first_chunk::<N>` and the first half of
`split_first_chunk::<N>` have fixed extent. Optional fixed slices retain this
bound through `?` and standard option access. Fixed extent does not admit
variable-size child comparisons, copies or hashes. Fixed array slots do not admit variable-size child comparisons.
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

Use `ctx.admit_iter(source, operation)?` before adapting an input-sized
source. The source is consumed and its exact visit bound is charged before
the first visit. `IterSource` is implemented by core alone: borrowed slices,
vectors, boxed slices, arrays, options, text, queues, B-tree maps and sets
and JSON maps yield references; mutable slices, vectors, arrays and B-tree
maps yield mutable references; owned vectors, arrays, options, B-tree maps and
sets and JSON maps move their values; `&DialectLayers` yields its primary and
extra layers. `Range` and `RangeInclusive` support `u8`, `u16`, `u32`, `u64`,
`u128` and `usize`, by value or by reference. Range admission checks the exact
remaining visit count before the first visit. A count that exceeds `u64`
refuses and fuses the caller budget. Text admission counts bytes for both
byte and character traversal. Hash tables are not sources. An iterator size
hint does not establish admission. `AdmittedIter` owns one traversal; its
source cannot be extracted or cloned. Map-like adapters with checked callbacks
keep the admission; filters, skips and `step_by` keep it only over an admitted
or constant-count base; `cloned` keeps it for Copy or fixed-clone items;
`chain` keeps it when both sides are admitted; `zip` keeps it when one side is
admitted and the other advances in constant work; `flatten` and `flat_map`
need a constant-count inner iterator, such as `char::to_lowercase`. A nested
loop requires its own admission. Child copies, comparisons and callback work
require their own operations. Dropping consumed or remaining values is free.
A search that can stop early (`any`, `all`, `find`, `find_map`, `position`,
`rposition`) over an admitted iterator is reported: the admission charged
every visit. `any_by`, `all_by`, `find_by`, `find_map` and `position_by`
step a fixed-step source and charge each step as it is made, including the
end probe of a search that finds nothing; `rposition_by`
searches a slice from the end the same way. Code whose errors are
`ResourceLimit` uses `all_by_limit`, `partition_point_limit` and
`equal_bytes_limit`, which charge the same way.

The standard reflexive `From<T> for T` and its `Into<T>` forwarding move the
value. They require no byte scan or allocation admission. A conversion between
fixed-size `Copy` values requires a proven fixed body. A `Copy` result alone
does not prove that its constructor performs fixed work.

A source of unknown length, such as XML children or a `from_fn` closure, is
stepped with `while let Some(item) = ctx.next_charged(&mut source, operation)?`,
which charges one unit before every `next`, including the end probe. Each
`next` must do constant work: a standard one-step iterator, adapters over an
admitted base, or a checked closure. A filtering adapter over a raw slice can
scan any number of items in one step and retains a finding. Core collectors
step their sources the same way.

`DecodeCost` states the bytes read by a hash or comparison, including owned
children. Core implements standard values. An owning crate implements its
records beside their declarations. The checker checks every implementation
as a decode root. Measuring variable children charges their traversal.
Key receipts match the exact operand and, for a tree, its comparison-depth
bound. Each receipt is consumed once. Mutation invalidates it. Map and set
growth also admits each stored key and its bytes before rehashing.
Single-character string growth admits the character's UTF-8 length as work
and storage. Work receipts match the character. Storage receipts also
identify the output string. Each receipt is consumed once and is invalidated
by mutation. Scoped text uses the same operation inside
`reservation.with_storage`.
Range receipts identify the range kind and each bound. A move receipt for a
vector's suffix from an index pays for `Vec::insert` at that index.
Truncating or clearing a collection needs no receipt.

Decode code uses `HashMap` and `HashSet` only for keyed lookup, insertion
and removal. Iteration order is unspecified, so decoded output must not
depend on it, and a scan walks the allocated table, which has no exact public
bound once removals leave deleted slots. Traversal (`iter`, `keys`, `values`,
`values_mut`, `into_keys`, `into_values`, `IntoIterator`, `for` loops,
including a loop that charges each step, and a table passed to any
`IntoIterator` parameter such as `extend`, `from_iter`, `zip`, `flatten` or a
core collector), the
set relations (`difference`, `intersection`, `union`, `is_subset`,
`is_disjoint`), the set operators `|`, `&`, `-` and `^`, whole-table `==`,
`clone`, `retain`, `drain` and `extract_if` are reported; a collection that decode traverses or compares is a `BTreeMap`,
`BTreeSet` or `Vec`. A keyed operation hashes and compares the key through its
`Hash` and `PartialEq<Self>` callbacks, and core charges the key's `DecodeCost`
for them. Standard keys (scalars, strings, `Cow<str>`, `Box`, `Vec`, `Option`,
slices, arrays and tuples) and keys whose `Hash` and `PartialEq` come from a
derive macro's expansion are accepted; a hand-written callback, or an
`automatically_derived` marker written by hand or by a declarative macro,
retains a finding at the lookup, insertion or removal, because std calls the
callback where the call graph cannot follow it. A keyed operation whose key,
query or builder is a type parameter is proven at each concrete instance and
reported at the instantiation that fails. `table[key]` through `ops::Index`
hashes or compares the key without a charge and is reported with
`get_hash_map` or `get_btree_map`. A B-tree key operation requires a
standard key, a key whose `Ord` comes from a derive expansion, or
`serde_value::Value`, which orders by variant and then by content.
Probing is not charged per probe, so the table must use `RandomState`; a
fixed-key builder such as `BuildHasherDefault<DefaultHasher>` lets an input
choose colliding keys and retains a finding. A removal leaves a deleted slot
that `capacity()` no longer counts, so a table can hold more buckets than
`capacity()` implies. Hashbrown reallocates only when the new length exceeds
half the real capacity, to at most twice the new length, so hash growth
charges the storage for twice the new length as work, which bounds the old
table it walks, and holds it as a scoped reservation while the table grows,
which bounds the new table before it is allocated. Right after growing, the
table has no deleted slots, so growth then charges the storage of its new
`capacity()` less that of the old one as retained bytes. Std exposes neither
the bucket count nor the deleted slots, so after removals that charge cannot
tell an in-place rehash from a doubling and charges as if the table doubled.
Under churn the retained total therefore exceeds the table's storage, by at
most 8/7 of an entry and its control byte per removal: no more than keeping
the removed entry would have retained. An in-place rehash
allocates nothing; the charged removals and insertions since the last rehash
pay for its walk, so a raw removal is reported with `remove_hash_map`,
`remove_entry_hash_map` or `remove_hash_set`. Growth then
charges the rehash of every stored key: length times a fixed key cost, or one visit
per key plus the sum of each variable key's `DecodeCost`, charged as one
amount so that the visit order cannot move a refusal. `unique_index` maps a
key that occurred once to `Some` and keeps a `None` tombstone for a repeated
key; uniqueness is tested through `get`, not `contains_key`.

A scoped storage receipt names its live reservation local. Moving that
reservation into another owner invalidates the local receipt. Keep the
reservation local through raw allocation and growth, then transfer the lease
to its returned owner. A wrapper does not establish a new storage receipt.

The operation table gives the core method for each listed shape. A replacement
message names an operation; it does not establish a missing implementation or
cost bound. Unknown custom conversions, callbacks, builders and external
allocators remain unproven until their concrete body and checked bound are
available. Admission primitives do not waive that proof. Core methods are on `DecodeContext`; ZIP entry methods belong to
`cadmpeg_container::ArchiveSnapshot`. Callbacks admit their child work. Core callback
parameters defer that obligation to concrete callers. An opaque callback
retains an `unproven_decode_charge` finding.

Generic core collectors charge each source step before advancing, including
an end probe. Their source contract is checked at callers. A standard
fixed-step source can be consumed directly. A filtering, skipping or nested
source must use adapters over admitted bases. A size hint can select storage
capacity; it cannot admit work. An opaque source keeps a caller finding.
Compiler-verified key and move receipts retain their operation-site identity
when generic bodies are imported into another checked crate. A receipt
admits that site alone.

Imported generic proof tracks each concrete instance with its fixed and paid
operands. Chain depth is bounded by the compiler recursion limit; distinct
states are bounded by its checked square. An unavailable body, expanding type
or exhausted bound retains an unproven finding.

| Operation shape | Core method |
| --- | --- |
| `for loop` | `admit_iter` on the base before adapters |
| `collection growth outside core operation` | `push_vec`, `reserve_vec`, `append_retained`, `push_retained_char` or the receiver-specific map/set insertion method |
| `into` | `copy_retained_text` for text; `copy_slice` for Copy slices; `into_boxed_slice` for an owned vector |
| `comparison` | `equal_bytes` for byte equality; `equal` for value equality; `compare` for ordering; whole hash tables are not compared, use `BTreeSet` or `BTreeMap` |
| `insert` | `insert_vec` for indexed vector insertion, inside `with_storage` for scoped storage; `insert_hash_map`, `insert_btree_map`, `insert_hash_set` or `insert_btree_set` for keys |
| `any` | `any_by` for slices; `admit_iter` before an iterator consumer |
| `contains` | `contains_text`, `contains`, `contains_hash_set` or `contains_btree_set`, selected by receiver |
| `get` | `get_hash_map`, `get_btree_map`, `get_hash_set` or `get_btree_set`, selected by receiver |
| `format!` | `format_retained` |
| `contains_key` | `contains_key_hash_map` or `contains_key_btree_map` |
| `clone` | `copy_retained_text`, `copy_slice` or `collect_vec` with charged child construction |
| `collect` | `collect_vec`, `try_collect_vec`, `try_collect_scoped_vec`, `collect_text`, `collect_scoped_text`, `collect_hash_map`, `collect_hash_set`, `collect_scoped_btree_map` or `collect_btree_set` |
| `all` | `all_by` for slices; `admit_iter` before an iterator consumer |
| `trait implementation unresolved` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `find` | `find_by` for slices; `find_text` or `find_bytes` for text/bytes; `admit_iter` for iterator consumers |
| `loop` | `charge_work` before each iteration path |
| `count` | `count` or `admit_iter` before the iterator consumer |
| `to_owned` | `copy_retained_text` or `copy_slice`; use `collect_vec` for owned children |
| `to_string` | `format_retained` |
| `external operation temporary or result storage` | `reserve_scoped` for a checked temporary bound, or `collection_vec`/`copy_retained_text` for caller-owned output; opaque allocation stays unproven |
| `eq_ignore_ascii_case` | `eq_ignore_ascii_case` |
| `parse` | `parse_text` |
| `extend` | `extend_vec` for vectors, options, arrays and borrowed Copy slices; for strings, `collect_text`, or `admit_iter` on the base, then `push_retained_char` for each character or `append_retained` for each text item |
| `get_mut` | `get_mut_hash_map` or `get_mut_btree_map` |
| `try_fold` | `fold` for slices; `admit_iter` before iterator consumption |
| `Display output extent unresolved` | `format_retained` |
| `attribute` | `xml_attribute` |
| `retain` | `retain_vec` for vectors; `retain_btree_map` or `retain_btree_set` for B-trees; hash tables are not scan-retained |
| `cmp` | `compare` |
| `sum` | `sum` for slices; `admit_iter` before a standard scalar consumer |
| `entry` | `entry_hash_map` or `entry_btree_map`; subsequent insertion uses typed storage admission |
| `push_str` | `append_retained` |
| `dedup` | `dedup_vec` |
| `strip_prefix` | `strip_prefix` |
| `to_value` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `from_utf8` | `validate_utf8` |
| `position` | `position_by` for slices; `admit_iter` before an iterator consumer |
| `fold` | `fold` for slices; `admit_iter` before the iterator consumer |
| `extend_from_slice` | `extend_from_slice` |
| `find_map` | `find_map` for slices; `admit_iter` before an iterator consumer |
| `rsplit_once` | `rsplit_once` |
| `trim` | `trim_text` |
| `clear` | `clear_vec` |
| `serialize` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `deserialize` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `Clone element layout unresolved` | `collect_vec` with a concrete charged child factory |
| `remove` | `remove_hash_map`, `remove_btree_map`, `remove_hash_set` or `remove_btree_set` |
| `vector collection storage reuse` | `collect_vec` |
| `nth` | `admit_iter` |
| `copy_from_slice` | `copy_into` |
| `split_once` | `split_once` |
| `derived Default` | `collect_indexed_vec` |
| `eq` | `equal_bytes` for bytes; `equal` for complete values |
| `cloned iterator child copies` | `copy_retained_strings` |
| `to_vec` | `copy_slice` for Copy values; `collect_vec` with charged child construction |
| `generic instantiation arguments cannot be normalized` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `strip_suffix` | `strip_suffix` |
| `min` | `min` for slices; `admit_iter` with charged comparison for iterator consumers |
| `to_writer` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `append` | `append_vec` |
| `conversion implementation unresolved` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `dedup_by` | `dedup_by` |
| `has_tag_name` | `xml_has_tag_name` |
| `reverse` | `reverse` |
| `dedup_by_key` | `dedup_by_key` |
| `join` | `join_retained` |
| `truncate` | `truncate_vec` |
| `make_ascii_lowercase` | `make_ascii_lowercase` |
| `root_element` | `xml_root_element` |
| `to_ascii_lowercase` | `to_ascii_lowercase` |
| `min_by_key` | `min_by_key` |
| `starts_with` | `starts_with` |
| `into_owned` | `copy_retained_text` for a borrowed text variant; move an owned variant |
| `with_capacity` | `collection_vec` for vectors; `retained_string` or `scoped_string` for strings |
| `binary_search` | `binary_search` |
| `rposition` | `rposition_by` for slices; `admit_iter` before an iterator consumer |
| `partition_point` | `partition_point` |
| `from` | `copy_retained_text` or `copy_slice`, selected by source and result |
| `deserialize_any` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `max_by` | `max_by` |
| `from_value` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `max` | `max` for slices; `admit_iter` with charged comparison for iterator consumers |
| `to_ascii_uppercase` | `to_ascii_uppercase` |
| `Clone implementation unresolved` | `collect_vec` or `copy_retained_text`; resolve and check the concrete Clone body |
| `drain` | `drain_vec` |
| `from_str_radix` | `parse_radix` |
| `min_by` | `min_by` |
| `is_disjoint` | `is_disjoint_btree_set`; hash sets are not traversed |
| `fill` | `fill` |
| `max_by_key` | `max_by_key` |
| `replace` | `replace_text` |
| `update` | `charge_work` for hashed source bytes |
| `shrink_to_fit` | `shrink_vec` |
| `is_ascii` | `is_ascii` |
| `make_ascii_uppercase` | `make_ascii_uppercase` |
| `sort_by` | `stable_sort_by` |
| `into_boxed_slice may shrink/reallocate: capacity equality unresolved` | `into_boxed_slice` |
| `trim_end_matches` | `trim_end_matches` |
| `end` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `external operation missing summary: annotations::_::_serde::Serialize::serialize` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `generic instantiation contains an indirect call` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `from_utf8_lossy` | `copy_retained_lossy_utf8` |
| `decode` (`encoding_rs`) | `charge_work` for the source bytes, then `decode_to_string_without_replacement` into `retained_string` output; it writes only within the admitted capacity and reports when the output is full |
| `ends_with` | `ends_with` |
| `last` | `admit_iter` |
| `get_key_value` | `get_key_value_hash_map` or `get_key_value_btree_map` |
| `trim_matches` | `trim_matches` |
| `trim_start` | `trim_start_text` |
| `is_subset` | `is_subset_btree_set`; hash sets are not traversed |
| `owning vector iterator collection may move or copy` | `collect_vec` |
| `binary_search_by_key` | `binary_search_by_key` |
| `trim_start_matches` | `trim_start_matches` |
| `trim_end` | `trim_end_text` |
| `new` | `collection_vec`, `copy_retained_text` or `parse_json_value`, selected by constructor; fixed empty constructors need no admission |
| `alloc_filled reaches resize child Clone` | `alloc_filled` for Copy values; `collect_indexed_vec` for charged child factories |
| `unzip` | `unzip_vec` with owned pairs or adapters over an admitted base |
| `custom` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `sort_unstable_by` | `sort_unstable_by` |
| `from_str` | `parse_text` |
| `push` | `push_retained_char` for strings; `push_vec`, `push_heap` or `push_back` for the concrete collection |
| `pop` | `DecodeContext::pop_heap` for a binary heap; vector and deque pops have fixed work. |
| `rfind` | `rfind_text` or `rfind_bytes`; `admit_iter` before reverse iterator search |
| `resize` | `resize_with` |
| `to_str` | `validate_utf8` |
| `remove_entry` | `remove_entry_hash_map` or `remove_entry_btree_map` |
| `try_for_each` | `admit_iter` |
| `binary_search_by` | `binary_search_by` |
| `decompress` | `begin_expand` and `charge_work` for compressed bytes; shared container inflate operations own the scan |
| `external operation missing summary: draft::ArenaEntity::arena_mut` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `rotate_left` | `rotate_left` |
| `resize_with` | `resize_with` |
| `for_each` | `admit_iter` |
| `split_off` | `split_off_vec` |
| `write_str` | `append_retained` for strings; `format_retained` for formatting output |
| `write_char` | `push_retained_char` for strings; `format_retained` for formatting output |
| `extend_from_within` | `extend_from_within` |
| `alloc_filled child Clone` | `alloc_filled` for Copy values; `collect_indexed_vec` for charged child factories |
| `to_uppercase` | `to_uppercase` |
| `shared owned copy` | `charge_retained` for the checked shared header and payload bound, plus `charge_work` for copied bytes; the shared allocator requires concrete proof |
| `hash` | `hash_value` |
| `from_utf16_lossy` | `utf16le_lossy_text` |
| `sort_by_key` | `stable_sort_by_key` |
| `resize child Clone` | `resize_with` |
| `parse_with_options` | `parse_xml` |
| `external operation missing summary: std::convert::From::from` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `sort_unstable` | `sort_unstable_by` |
| `to_lowercase` | `to_lowercase` |
| `trim_ascii_end` | `trim_ascii_end` |
| `rotate_right` | `rotate_right` |
| `build` | `reserve_scoped` and `charge_work` for a concrete bounded builder; opaque builders remain unproven |
| `retain_mut` | `retain_mut` |
| `deserialize_map` | `parse_json` for derived decode trees; `parse_json_value` for value trees. Rebuild serialized owned fields with `collect_vec` and `format_retained`; custom Serde calls remain unproven |
| `clone_from` | `copy_retained_text` or `collect_vec`, then replace the owned value |
| `make_mut` | `copy_retained_text` or `collect_vec` to construct the replacement; shared copy-on-write storage requires concrete proof |
| `lzma_decompress_with_options` | `begin_expand`, `charge_work` and `reserve_scoped`; the concrete decoder workspace bound must be established |
| `decode_to_utf8_without_replacement` | `collection_vec` for output slots and `charge_work` for the input; bounded slice decoder |
| `decode_to_string_without_replacement` | `retained_string` for the caller's output capacity and `charge_work` for the exact source bytes; the pinned decoder writes only spare capacity |
| `decode` | `collection_vec` and `charge_work`; replace allocating decoders with their bounded slice forms |
| `for_label` | `charge_work` for label bytes |
| `splice` | `splice_vec` |
| `reduce` | `admit_iter` |
| `replace_range` | `replace_text_range` |
| `decode_slice` | `collection_vec` for output slots and `charge_work` for the encoded input |
| `by_index_raw` | `ArchiveSnapshot::new` and its admitted central-directory traversal; borrowed metadata avoids payload expansion |
| `decompress_stream` | `open_zstd`; its reader owns each admitted stream step |
| `find_frame_compressed_size` | `charge_work` for compressed frame bytes |
| `external operation missing summary: dialect::FormatIdentityPayload::format` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `external operation missing summary: codec::Codec::decode_with_context` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `external operation missing summary: document::EntityRewrite::rewrite` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `external operation missing summary: schema::EntitySchema::identity` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `external operation missing summary: features::patterns::CompositeStages::try_map_stage_lengths_owned` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `external operation missing summary: geometry::nurbs::PoleValue::admit_surface_poles_for_decode` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `copy_within` | `copy_within` |
| `digest` | `charge_work` for hashed source bytes |
| `external operation missing summary: native::canon::ByteSink::write_bytes` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `write_all` | `charge_work` for source bytes; use `begin_expand` when the target owns expanded storage |
| `sort` | `stable_sort_by` |
| `by_index` | `ArchiveSnapshot::probe_readable_names` for tolerant name probes; `ArchiveSnapshot::new` and `ArchiveSnapshot::open` for payloads; expansion uses `begin_expand` |
| `external operation missing summary: std::f64::<impl f64>::log2` | `charge_work` for a resolved operand bound and `reserve_scoped` for checked temporary bytes; resolve the concrete implementation before admission |
| `custom comparison work` | `equal` or `compare`; the concrete comparison body must admit child work |

Unicode case conversion admits `n*(n+1) + 48*n + 16` work units,
`24*n + 8` retained bytes and `36*n + 16` temporary bytes for `n` UTF-8 input
bytes. The bound includes contextual sigma scans, at most three four-byte
scalars per input scalar, geometric capacity and coexisting relocation buffers.
One live receipt binds the input and workspace to one standard case conversion.
Replacement text uses the existing substring search and retained append operations.

Sorting measures the maximum projected key cost. The admission bound uses
that maximum for both operands of every possible comparison. A sum of key
sizes cannot cover repeated comparison with one large key. The projection
must identify the same key throughout the operation.

The formatting family charges each fragment before counting or appending
it. Standard string and numeric `Display` work is proportional to the
written bytes. Custom formatters in checked crates remain checked bodies.
Arguments to `format_args!` do not require a separate length bound.

`Grammar::of(ctx, dialect)` copies the local grammar name with
`copy_retained_text`. `DialectMatch::grammar_id(ctx, grammar)` formats the full
identity with `format_retained`. `DialectMatch::using(ctx)` delegates to that
operation only for an unverified admission. All three return the original
resource refusal. Serialization builds its text outside decode admission.

`Deserialize` and `Serialize` implementation bodies are Serde callbacks.
They are excluded from decode body analysis. Deserialization admission is
checked at the decode call. `parse_json` accepts a derived type tree: each
contained nonstandard type must have deserialization derived by `serde_derive`.
An `automatically_derived` marker alone does not establish this origin.
`parse_json_value` owns the value-tree parser bound. A custom deserializer,
a contained custom deserializer, or deserialization outside these operations
retains an `unproven_decode_charge` finding at the calling site. Serde
callbacks carry no hidden or thread-local context.

Derived deserialization with `try_from`, `from`, `with`, `deserialize_with`,
`default`, `skip`, `skip_deserializing`, `flatten`, `untagged`, `tag` or
`content` can call custom code or scan buffered values again. `remote` derivation also requires a separate
callback proof.
Derivation alone does not prove those callbacks. Decode reads a plain wire
type and applies its context-taking constructor afterward.

Typed JSON maps and sets require standard scalar or String keys. Hash
containers also require the standard RandomState hasher and Global allocator.
Custom key hashing, equality, ordering and hasher construction remain unproven.
A JSON parse pass, the bounding parse and the typed validation alike, charges
the input length times two plus one key search: a scan, a copy of each string
and the comparisons of one B-tree search path, the smaller of the entry count
and the per-level search bound. A `raw_value` carrier can replay its string at
every nesting level and charges the input length times the entry count and
the nesting depth. The conversion from the parsed tree charges one step per
value plus the key comparisons. The validation result uses scoped storage;
the conversion result uses retained storage.

The target proof bounds container storage, including live old and new buffers,
against the typed parser allowance. Inline element size and container layout
use the target architecture. A sequence member supplies one value allowance
for each JSON node it must occupy: its own node and one for each required
field of a derived record. Recursive targets must consume a JSON value
before revisiting the same type. Transparent, Option and pointer wrappers do
not establish that progress.

Serde deserialization is context-free reconstruction. A `TryFrom<String>`
callback selected by `#[serde(try_from = ...)]` keeps its signature and uses
standard allocation. It is not a decode body. Decode reads the raw wire value
and passes the caller's `DecodeContext` to its constructor. Validation has one
implementation parameterised by a typed admission. Decode admission charges
the caller and returns the original refusal. Context-free admission has the
failure type `Infallible`.

`schema::structural::project` charges each node it emits, the text it copies,
its scoped storage, nesting depth and the charged B-tree lookups of map
entries. The source's `Serialize` implementations are Serde callbacks outside
body analysis, so the checker reports each project call whose concrete source
reaches a `Serialize` implementation; a custom implementation that copies or
scans before emitting, such as a `to_vec`, a base64 encoding or a
`#[serde(into)]` clone, is the work those findings protect.

A shared `const fn` grammar validator has one implementation for constant
construction and runtime decode. The runtime caller charges the scanned extent
through its context immediately before calling the validator. Constant callers
keep their existing signatures and perform no runtime admission.

A checked context operation owns its work admission. Its call site carries
no duplicate body finding. An unavailable or unresolved implementation
uses the third rule. Arguments and callbacks remain checked. Custom
comparison implementations are inspected separately from their owning types.

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
by the compiler integration suite. Fixtures that link the real core, container
and IR crates read the MIR of their non-generic functions, so the dependencies
are built with it encoded:

```
RUSTFLAGS=-Zalways-encode-mir cargo +nightly-2026-09-08 test -q --manifest-path crates/cadmpeg-decode-policy/Cargo.toml --lib
```

A refusal test finds a named boundary with
`cadmpeg_core::decode::refusal_probe::RefusalProbe` (core feature
`test-support`, enabled by `cadmpeg-test-support`). It leaves the dimension
unlimited in the policy of the context under test and arms the probe; the
first charge of the operation that would raise that context's peak usage
refuses, reporting the peak as the limit. One decode finds the boundary, and
`cadmpeg_test_support::refusal::resource_limit_at` replays it one unit below
the need under the ordinary limit. A decode charges in the same order on every
run, so a replay that refuses elsewhere fails the test: it exposes a charge
sequence that depends on unordered iteration.

Heap sifts charge the maximum `DecodeCost` of all stored operands and the incoming
value. With `n` operands, the work bound is `n` measuring visits plus
`(bit_length(n) + 1) * 4 * (maximum_operand_bytes + size_of::<T>())`.
Heap capacity growth has its own retained, scoped and movement charges.
Hash growth charges the storage for twice the new length as movement work and
holds it as a transient scoped bound, in addition to the rehash of stored keys,
and charges retained storage from the table's real capacity once it has grown.

A B-tree of h levels holds at least 2 * 6^(h-1) - 1 entries. A tree lookup
charges the key's cost for at most eleven comparisons per level, and never more
comparisons than stored keys. Mutation work is counted in node passes, one
pass being the node byte bound. A split, a merge and a steal each cost at most
two passes. Every split creates a node, and a tree of n entries holds at most
(n - 1) / 5 + 1 nodes, so the splits so far are at most the increases of that
bound over the insertions so far plus the nodes that merges have freed. An
insertion therefore charges one shift and two passes for each node its length
adds to the bound. A removal charges one shift, one steal and, for every level
of the tree, a merge and the split that may later recreate its node; a tree of
at most ten entries is one node and a removal only shifts it. Std merges an
underfull node whenever the result fits, so alternating insertions and
removals can split and merge a whole path each time; the per-level removal
charge pays for that. Insertion charges no work proportional to the stored
length.

A sort's charge does not depend on the input's order, so a decode that sorts
values gathered in an unspecified order still charges deterministically.
`is_sorted_by` compares neighbours, each comparison charged one step and both
operands' key costs, and stops at the first pair out of order; code whose input
order is deterministic uses it to skip a sort. A stable sort of twenty or fewer
values inserts by adjacent swaps and compares every earlier neighbour without
stopping where the value comes to rest, so its steps depend only on the length.
A stable sort of more than twenty values sorts an index array, whose sort
admits the comparisons and index moves once, and then moves each value along
its permutation cycle, admitting two value moves per value. It charges one unit
per swap and one per value not swapped, so the total and the refusal point
depend only on the length. Truncating, clearing, filling or
compacting a vector charges nothing for the values it releases.
