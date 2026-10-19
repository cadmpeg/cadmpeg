# Illegal-state conversion reconciliation

## Decision

Keep `main`'s decode architecture and resource policy. Preserve domain invariants
from `feat/illegal-states-conversion` and its history through small,
independently tested ports.
A model invariant describes valid CAD data. A resource limit controls how much
work an implementation accepts. These are separate contracts.

This reconciliation starts at `157938df1`, the merge of
`investigate/f3d-budget-186-187` in PR #208, and is rebased onto `7bcc95276`,
the subsequent automated workspace version bump to 0.6.2.
The reviewed conversion branch ends
at `78ee1b28c1fb9306817de7f338aacc62851a3589`. This is a selective port, not a
merge of that branch or a claim of behavioral equivalence with it.

## Preserved contracts

| Concern | Decision |
| --- | --- |
| Model admission | Keep checked finite scalars, quantity types, knot order and cardinality checks, rational-pole structure, identity grammar, and topology carriers already present on `main`. These types enforce their declared local invariants. |
| Source recovery | Keep independent readable content. Omit the smallest dependent record whose boundary is known and report its loss. A contradiction must not become an absent value or an invented replacement. |
| Decode depth | Keep container, geometry, and full decode states. Encoders request their required depth; explicit full decode and loss rejection retain their existing behavior. |
| Validation | Keep complete findings in reports and use the planned output's arena coverage to decide whether conversion can proceed. |
| Session ceilings | Keep input bytes, decompressed bytes, retained bytes, admitted entities, and recursive depth. Exhausting a session ceiling refuses the decode. |
| Local work | Keep bounds owned by individual searches and recovery algorithms. Exhausting a local bound skips that item with a loss; it does not poison the session. |
| Serialization | Keep the current CADIR and report shapes. These ports add no fields, variants, resource dimensions, or version changes. |

Checked types do not prove whole-document validity. Reference resolution,
cross-record topology, and geometric consistency still need runtime
validation. Keep those checks alongside construction-time admission.

Do not restore cumulative collection-item or work-unit counters, temporary
materialization accounting throughout model operations, or the conversion
branch's compiler-based accounting checker. Those mechanisms couple ordinary
model operations to a session-wide refusal policy. They reintroduce the policy
that PR #208 replaced.

## Ports

| Source commit | Defect on the base | Preserved behavior |
| --- | --- | --- |
| `3be69c70fd5468ad99d1e5315f03dbbb57f9f90d` | Creo function 10 converts a conflicting auxiliary equality into an absent equality. A stored zero then permits an unjustified coordinate derivation. | Reject the derivation when either auxiliary component conflicts. Keep the accepted absent-equality and consistent-zero cases. |
| `22043953ccd6b9ec14f964774ec086c55a31c123` | Rhino projected-trim reversal forms `start + end` before reflecting knots. That sum can overflow while all reflected knots remain representable. | Use the existing checked IR reflection operation when the sum overflows. Preserve finite-sum arithmetic, rational weights, source geometry, and rejection of unrepresentable outer knots. |
| `f9e49288b985fb8469fff7a7f5f9e9f381270ef9` | Homogeneous Bézier midpoint subdivision forms an overflowing endpoint sum for finite constant curves. | Use the standard scalar midpoint operation. Preserve extreme and subnormal constant controls and the zero midpoint of opposite extremes. |

The Creo fix remains in the reviewed branch tip. The Rhino fix was reverted
by `2d5c8c86282d70f399766116b8fb1793e4e38e0e` to remove writer changes from
the decode-budget pass. The Bézier fix was reverted by
`51f10e68388489fc6525a65bd4768464a8ab5977` in an audit that removed work
outside budget accounting. This reconciliation deliberately restores those
two numerical fixes from history. Both defects reproduce on the base, and
their corrections require no accounting machinery or wire changes.

The source tests are adapted to `main`'s context-free model APIs. Their
geometric and reconciliation assertions remain the reason for each port;
resource-accounting setup is not part of those invariants.

## Existing fixes retained

The base already contains nonzero-root CFB stream addressing, complete Creo
structural compact-token admission, finite STEP real-token admission, Protein
distance-overflow rejection, and ambiguity checks for Inventor default
renderings. They need no replacement from the conversion branch.

The base's NX probability decoder applies the symbol bias in a wide integer
before signed-range admission. It accepts raw values `0x80000000` and
`0x80000001` as `i32::MAX - 1` and `i32::MAX`. The conversion branch's earlier
overflow fix rejects both values. Keep the base's implementation and tests.

Keep the base's finite-sum algorithms, source-fidelity storage, procedural
surface attachment, and geometry-depth product prerequisites. Broad evaluator
additions, API rewrites, and accounting-only test suites are outside this port.
They require their own behavior specification and regression evidence before
adoption.

## Verification

Each imported regression must fail on the unmodified base and pass after its
fix. All five imported regressions were run in both states: five failures
before the fixes and five passes afterward. Run type checks before tests,
then the repository pre-commit gate over the changed crates and their
dependents. The shared Bézier change requires the
workspace dependency closure. Keep the existing golden artifacts unchanged;
any difference requires investigation, not automatic regeneration.

The final gate passed across 19 crates and 37 test targets: 17,804 tests
passed, none failed, and three existing tests remained ignored. The ignored
tests require F3D history reference data, an explicit 400-state history run,
or an openNURBS reader installation. Formatting, Clippy, dialect checks,
source policy, resource-cap census, and deserialization census all passed.
Golden artifacts are unchanged. The gate used the repository's CI settings
for disabled incremental compilation and debug information, with two build
jobs. This gate ran before the version-only rebase. Every Rust source and
test file is identical after the rebase. The manifest and lockfile comparison
confirms that only workspace package versions changed. Locked metadata
resolution and affected-crate type checks with tests passed after that
update.

The authoritative project contracts remain [architecture.md](architecture.md),
[admissibility-routes.md](admissibility-routes.md), and
[decode-resource-caps.md](decode-resource-caps.md). The destination remains
full semantic CAD interoperability, including native writing and explicit
loss reports, as defined in [roadmap.md](roadmap.md).
