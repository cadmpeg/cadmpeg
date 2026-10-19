# Illegal-state conversion reconciliation

## Determination

Use `main` as the architecture. Preserve checked CAD values and independently
useful algorithms from `feat/illegal-states-conversion`. Do not restore its
per-operation accounting system or replace main's recovery and export policy.

The two goals are compatible: make invalid local CAD values unrepresentable,
and recover every independently readable part of a native document. They become
incompatible when every string comparison, collection operation, or temporary
model value can exhaust a shared session work counter. That coupling is the
part rejected here.

The earlier three-fix reconciliation was incomplete. This record supersedes
its scope and verification claims. The reconciliation now includes shared IR,
I/O, compression, layout generation, seven codecs, serialization, and tooling.
It also explicitly retains main's improvements across the remaining codecs,
validation, native writing, CLI, and resource policy.

## Compared revisions and coverage

| Revision | Commit |
| --- | --- |
| Common ancestor | `2f04d12e65ee0f66ffc7f863e5b1f5685fe7fa8a` |
| Main baseline, including PR #208 and the 0.6.2 version update | `7bcc95276548de4e4876894b9a650a84ea9c4d6d` |
| Conversion branch tip | `78ee1b28c1fb9306817de7f338aacc62851a3589` |
| PR #208 squash | `157938df13aa439b5b87fe7174521dee5b8ace58` |

The main-to-source comparison contains 4,515 distinct changed paths before
rename pairing. At 1,297 paths the source blob is identical to the common
ancestor, including paths absent from both. Those paths contain no source-side
change to recover. Another 1,429 paths changed only on the source side; 1,789
changed on both sides. These figures count paths, including tests and moved
files. They do not measure independent features.

[The disposition inventory](reconciliation/illegal-states-files.tsv) names every
one of those paths. `main-lineage` is an exact blob comparison. `main-policy`
is an explicit architectural choice, not a claim that two implementations are
identical. `adapt` preserves selected behavior on main's APIs. `source` takes
the source file. Each row names its rationale below or gives specific evidence.
Integration changes outside that difference set are listed separately in
[the integration inventory](reconciliation/illegal-states-integration.tsv).

Review used three-way file and declaration comparisons, caller inventories,
new-function and diagnostic inventories, source history, and executable
regressions. Source history matters: several valid numerical fixes were
reverted because they fell outside its budget-accounting pass. They belong in
this reconciliation. A mechanical merge or a copy of the source tip loses that
information.

## Contracts

| Concern | Reconciled contract |
| --- | --- |
| Local validity | Checked finite scalars, units, identities, cardinalities, knot sequences, rational poles, and tagged states belong in constructors and deserializers. |
| Document validity | References, ownership, cycles, orientation, and geometric agreement require graph and numerical validation. A checked field does not prove a valid model. |
| Recovery | Retain readable independent content. Omit the smallest dependent item with a known boundary and report the loss. Do not replace a contradiction with absence or an invented value. |
| Session ceilings | Input bytes, decompressed bytes, retained bytes, entities, and actual recursive depth remain fatal session limits. |
| Local algorithms | Searches and optional recovery retain their local bounds and attributable losses. Exhausting one does not refuse unrelated content. |
| Decode depth | Keep container, geometry, and full depth, encoder requirements, full-decode override, and loss-policy behavior. |
| Export | Keep complete findings and the planned output's arena coverage. Keep source replay, retained-content patching, and semantic synthesis distinct. |
| Wire compatibility | Preserve CADIR and report shapes and version 6. Borrowed serialization must emit the same fields, order, omissions, and numbers. |

Reject the source's collection-item, work-unit, and temporary-materialization
budget dimensions; charged comparison, sorting, formatting, and model APIs;
`DecodeCost` implementations; reservation receipts; refusal-probe suites for
those dimensions; and the rustc-private accounting driver. Their support files
and tests are rejected with the mechanism they enforce. This is a package-level
policy decision, not a claim that every wrapper is defective.

## Preserved and adapted changes

| Owner | Change and reason | Evidence |
| --- | --- | --- |
| IR value states | Use `NonBlankString` for selection references. Move owned revolve inputs between states without cloning whole selections. | Selection admission tests and all eight revolve states, with optional selections. |
| IR evaluation | Index surface patches by U row and V span. Preserve the first boundary winner. Borrow polyline samples and stop reading after the result is known. | Grid lookup compared with a linear oracle; selected-access and early-stop tangent tests. |
| IR pcurve evaluation | Carry finite, nonfinite, and unstated acceleration explicitly through placement and trimming. Retain main's evaluation refusal states. | Nested pcurve and numerical audit tests. |
| IR serialization | Serialize polar NURBS and sketch patterns through borrowed views. Preserve exact JSON and deserialize through existing checked wire types. | Exact byte comparisons, optional-field combinations, Unicode identities, signed zero, and round trips. |
| IR membership | Use a tree set for sketch identity uniqueness, avoiding repeated sorted-vector shifts. | Existing duplicate-member and sketch admission suites. |
| IR comparison | Stream IGES record comparison, normalize D exponents only when needed, and bound JSON diagnostic previews at UTF-8 boundaries. | Comparison and preview tests. |
| IR tessellation | Validate borrowed triangle lanes and count strip triangles without constructing a second triangle list. | Existing mesh construction, mutation, and strip tests. |
| IR numerical validity | Use overflow-safe Bézier midpoints. Use first and last NURBS poles as sketch endpoints only for exact endpoint clamping. | Extreme/subnormal midpoint regressions; clamped, unclamped, overclamped, and periodic sketch cases. |
| Core input | Reject a `Read` implementation that reports more bytes than the supplied buffer, including the EOF probe. | Two malformed-reader regressions. |
| Container compression | Reject trailing compressed bytes before publishing an exact-inflate arena space. Failed inputs cannot consume visible space identities. | Two invalid inflations followed by a valid inflation and space-index assertions. |
| CFB layout | Add the directory-entry layout specification and generated constants. Make generation resolve the owning module per format. | Layout arithmetic, citations, generated-code checks, and independent same-name read-set test. |
| STEP dependencies | Replace repeated retry-queue scans with compressed retry histories and dependency wakeups. Preserve FIFO successful-completion order and release completed wake lists. | 1,800 dependency graphs against the original queue oracle, repeated edges, cycles, long chains, and shared prefixes. |
| STEP units | Cache completed scale resolutions by unit kind and entity identity. Retain cycle checks, positive intermediate values, and actual recursion admission. Do not import a 256-link logical-depth cutoff. | Shared 1,024-unit chain, separate angle/length values, cycles, missing IDs, invalid factors, and existing depth-limit tests. |
| STEP presentation | Order scalar-conflict losses by source style and target order while retaining main's checked nonempty groups. | Three source-order regressions. |
| STEP topology | Offset face indices within each shell when assigning connected components. A later shell cannot overwrite an earlier shell's face ownership. | Multi-shell BREP-with-voids regression failed before the fix and passes afterward. |
| Creo scalar parsing | Bound helper reads by their enclosing section or record. Require complete compact-integer fields and terminators. Distinguish explicit null from missing/truncated solver values. | First eight regressions all failed before the fix; sixteen window regressions pass after it. |
| Creo family tables | Search and read the pointer inside the selected section; retain absolute source offsets. | Complete, partial, null, foreign-suffix, and absolute-position cases. |
| Creo equations | Reject function-10 coordinate derivation when auxiliary equalities conflict. | Contradictory, absent, and consistent-zero equality cases. |
| FreeCAD history | Build body-predecessor maps lazily from immutable objects. Preserve first usable body and first occurrence rules. | Lazy construction and duplicate/precedence fixtures. |
| Rhino B-rep | Index legacy endpoint coordinates lazily. Preserve first match, signed-zero equality, and nonmatching NaNs. | Linear-oracle float and duplicate cases. |
| Rhino extrusion | Reuse one exact NURBS conversion for orientation and both ends. | Existing orientation and extrusion tests. |
| Rhino trim reversal | Use checked knot reflection when the endpoint sum overflows; retain the finite-sum path. | Representable reflected knots, rational data, and unrepresentable outer knots. |
| CATIA boundaries | Prune segment pairs with an AABB index, then run main's exact predicate in its original order. Round query bounds outward and retain the local 4,096-point cap. | 1,000 random sets at three tolerances against brute force; ULP contacts; separated polygons require no exact pair tests. |
| NX blends | Compute operand binomial coefficients once for Bernstein products. Preserve multiplication order and finite-result checks. | Degrees 0–12 compared with independently evaluated polynomial products. |
| SolidWorks history | Memoize feature ancestry instead of repeatedly walking shared ancestors. Preserve source-parent/tree-parent rejection order. | Forward references, cycles, missing/duplicate IDs, error order, and 4,096-node shared ancestry. |
| Parasolid | Use a direct constant suffix check for the `_13006` schema form. | Existing schema grammar and classification suite. |
| Tooling | Restrict declaration matching in the unknown-field census so macro keyword lists are not treated as Rust items. Retain main's parser cache and bounded regex. | 111 checker tests, including the source regression. |
| Protein documentation | Correct checked property types and repeated-reference descriptions. | Matched against current public types; no decode behavior change. |

The history fixes include Creo `3be69c70f`, Rhino `22043953c`, Bézier
`f9e49288b`, and sketch endpoint clamping `6c3cacb4c`. The last three numerical
changes require no source accounting machinery.

## Baseline choices by subsystem

These decisions apply to each inventory row labelled `main-policy`, except
where a row gives more specific evidence or an adapted file overrides it.
Tests remain with main's production owners. Source-only test trees that exercise
rejected APIs are not copied into the reconciled tree.

### Core and containers

Keep `View`, stable arena ownership, directed prefix probing, input acquisition,
and the five session ceilings. Main's CFB probe already reads requested sectors
through `ProbeInput::sector/ensure`; the source's resumable-prefix state machine
solves repeated-prefix rescanning that this route does not perform.

Keep main's UTF-16 extent and surrogate rules, duplicate-key tombstones, empty
needle semantics, and `memchr` searches. Keep XML/JSON preflight and actual
parser-depth admission. Keep zstd admission based on the buffer the decoder
uses, with local unsupported-window recovery. Do not replace it with fatal
session accounting for the declared window and repeated whole-suffix work
charges. Exact inflate publication and malformed-reader checks are exceptions
ported above.

### Shared IR and validation

Keep main's checked geometry, feature, sketch, topology, native, identity, and
fidelity types. The source's extra admission traits, cost implementations,
charged deserializer, priority queue, validation scratch, equality wrappers,
and collection traversals implement the rejected accounting policy. Standard
collections and context-free checked constructors remain appropriate here.
The independent borrowed serializers and evaluator improvements are preserved.

Keep graph validation and native namespace isolation, including native byte
encoding rules. Keep main's higher-order surface requests; the final source
surface-request file is identical. Preserve main's procedural attachment
semantics: a rejected batch retains its accepted prefix and stops. The source
batch API continues after individual rejection and is not a compatible
replacement.

`ProceduralIndex` remains an append-only cache with a documented requirement:
existing identities, construction references, and arena order must not change
while it is live. It is not a whole-model immutability proof. Retain the
existing single-attachment, batch, reconstruction, and final validation routes;
do not claim that local checked types eliminate these graph obligations.

### ASM and SAT

Keep main's shared subtype resolution, checked knot carriers, support-chart
normalization, and topology projection. Main's `embedded_support` carries the
parsed chart with its surface; the source separately reconstructs
`NativeSupportChart` from tokens. Main memoizes subshell ancestry and guards
cycles. The source's owner-cycle helper changes traversal/accounting structure,
not the required shell-ownership contract.

Keep SAT detection and header recovery based on fields needed to interpret the
stream. Do not make descriptive record counts and metadata lines fatal integer
admission gates. Reject accounting wrappers and test rewrites with that policy.

### Fusion

Keep main's geometry-depth cut, product prerequisites, native validation,
parameter ordering, and native writer coverage. Source module splits route
byte fields, references, identities, history, and projections through charged
operations; importing their orchestration would restore the rejected session
policy. Main already orders parameter scopes with a local bound, deterministic
source-order fallback, and an explicit loss.

Keep writer arena dispositions, binding limits, and collapsed-edge/coedge
orientation logic. Source's simplified edge-use counting is not a replacement
for those checks. Keep main's documented local failures and history tests.

### Inventor and Protein

Keep main's shared expression measurement and bounded root-text emission.
The source's per-expression cache stores intermediate rendered strings; main
shares measurement across parameters and does not need those strings. Keep
existing color and ASM-face indexes, ambiguous rendering rejection, checked
external-reference identity, and minimum byte-extent checks for native counts.

Keep Protein's finite values, repeated-value types, property-content variants,
schema catalog, and nonthrowing schema-presence query. Do not introduce dual
standard/charged read routes to support the rejected accounting system.

### FreeCAD

Keep main's generic canonical typed-native comparison rather than importing a
second application-specific comparator. Keep geometry-depth behavior,
retained-document patching, source metadata recovery, and prototype-aware
product-cycle detection. Main filters eligible appearance bindings in the
transfer path; source moves that work into separate appearance-plan indexes.
Retain main's ownership and filter location. Preserve lazy body predecessors.

### Rhino

Keep main's streaming knot-gap insertion, wide-domain interpolation, scaled
means, and evaluation refusals. Do not replace them with source scratch and
charged traversal variants. Both branches already recover the first matching
annotation userdata. Preserve the independent endpoint index, extrusion reuse,
and checked reversal described above.

### STEP

Keep main's checked groups, graph recovery, metadata handling, and local limits.
Adapt dependency scheduling, curve-parameter scale memoization, unit scale
memoization, diagnostic order, and shell ownership. Cache results within the
immutable exchange being resolved; distinguish unit kinds; never cache a
resource refusal as an absent semantic value. Retain actual recursive-depth
checks even when a result is cached.

### IGES

Keep main's bounded direct geometry proofs, checked entity records, native
retention, and export policy. Do not import the source's proof-reuse system
across approximately equivalent plane placements: its tolerance cells,
neighbor probes, rounding envelopes, and translation lattice add a second
numerical acceptance path. Direct evaluation of the requested placement keeps
one proof path and the existing local work bounds. Source charged evaluation
copies, proximity tables, and fixtures belong to that alternative pipeline;
retain main's pipeline as a unit. This is an explicit implementation choice,
not a claim that the source's cache produces identical arithmetic.

### CATIA, Creo, NX, and SolidWorks

Keep main's checked native carriers, ambiguity states, local recovery bounds,
owner-aware topology, and transfer loss vocabulary. Reject source-wide
accounting context propagation, charged ownership/index construction, and
policy-specific test trees. Preserve the independent fixes and algorithms
listed in the port table.

Keep main's NX wide-integer symbol bias: raw values `0x80000000` and
`0x80000001` remain representable after bias. The source's earlier overflow
fix rejects them. Keep main's Creo truncated triangle-strip accumulator reset;
it already preserves the source correction. Keep complete structural token
admission and do not confuse it with the additional scalar-window fixes here.

### Registry, CLI, builds, and tests

Keep seekable-source identification, encoder `DepthNeed`, output arena coverage,
complete check reports, loss rejection, atomic output, dialect registration,
and CLI query/inspect contracts. Source removes or rewrites some of these while
threading accounting through the application. That is not the desired product.

Keep the pinned toolchain, scoped dependent-crate gate, source-policy rules,
resource-cap census, native namespace checks, golden floors, and current test
ownership. Do not import the rustc-private accounting driver, its fixtures,
its CI workflow, or its lockfile dependencies. Preserve the independent
unknown-field checker fix and CFB layout generator support.

## What cadmpeg is and what it requires

cadmpeg is a native CAD interoperability system. It reads vendor containers and
kernel records into a typed neutral document while retaining source-specific
records and bytes beside that document. The model covers exact geometry,
topology, tessellation, sketches, constraints, features, history,
configurations, assemblies, appearances, and annotations. Understanding the
shape is only one part of understanding the design.

The working pipeline is detection and inspection, decode, validation, export
planning, then writing. A dump only serializes the decoded model; it does not
certify that model. A writer can replay unchanged source bytes, patch retained
native content, or synthesize a new native representation. The report must
identify the path used and every loss. Unknown data can survive only when the
chosen edit and write path leave its meaning valid.

The long-term goal is full semantic interoperability: read native CAD, edit its
typed model, write valid native files, and translate between formats with
explicit losses. The roadmap uses SolidWorks parts as the reference path for
history completeness and native writing. It calls for a public CC0 corpus,
semantic equality independent of regenerated IDs, geometric error measures,
version coverage, independent-reader validation, and then assemblies and
external references beyond the part envelope.

The project therefore requires several kinds of proof:

- Byte specifications and bounded parsers establish where records begin and
  which fields control interpretation.
- Checked types prevent invalid local values from entering ordinary code.
- Graph and geometric validators establish ownership, identity, orientation,
  units, parameter charts, and cross-record consistency.
- Source retention and loss reports state what the conversion understands,
  preserves, approximates, or omits.
- Round trips, independent readers, public fixtures, and adversarial tests
  establish that generated files work outside cadmpeg's own implementation.
- Algorithms must scale with real parts and histories without turning one
  unsupported record into loss of the entire readable document.

The main challenges are proprietary format breadth, incomplete evidence,
non-equivalent feature semantics, floating-point range and tolerances,
periodic and trimmed geometry, topology ownership, history dependencies,
unknown-record validity after edits, and reproducible validation across
versions. The reconciliation serves those requirements by combining stronger
local invariants with bounded, explicit recovery and scalable algorithms.

## Verification

The implementation at `0fb9a6983ed9417e780321f0c1468b8c088a8517` passed the
repository pre-commit gate without bypassing hooks:

- Clippy passed across all targets in the 20 affected crates and their
  dependents.
- The complete test gate passed 18,013 tests across 38 targets, with no
  failures and three existing ignored tests. This includes CFB layout
  arithmetic, specification citations, and generated-constant verification.
- The separate IR and ASM `schema` feature run passed 2,162 tests.
- Python tooling discovery passed all 524 tests, including the 111
  unknown-field census tests.
- Formatting, source policy, resource-cap census, unknown-field census,
  dialect identity/support, codec facade, rendered capability tables, and
  documentation references passed.
- An independent no-rename Git diff check confirmed that the inventory
  contains all 4,515 differing paths exactly once. Every retained-main row
  matches its pinned main blob; every source row matches its pinned source
  blob.

The three ignored tests are unchanged: the F3D history snapshot oracle needs
`CADMPEG_F3D_HISTORY_ORACLE`, the 400-state F3D history scale test is opt-in,
and the independent Rhino reader test needs `OPENNURBS_ROOT` with an
`example_read` executable. These are not counted as passes.

Golden artifacts, dependency files, and wire versions are unchanged. No
snapshot was regenerated to accept an unexpected difference. The gate used
Rust 1.97.0, two build jobs, disabled incremental compilation, and disabled
development/test debug information. The follow-up verification record changes
documentation only.

All commits added to main by this branch use `pcurve` for author, committer,
and DCO sign-off. The repository-local Git identity and GitHub authentication
were also verified as `pcurve`.

The authoritative contracts remain [architecture.md](architecture.md),
[admissibility-routes.md](admissibility-routes.md),
[decode-resource-caps.md](decode-resource-caps.md), and [roadmap.md](roadmap.md).
