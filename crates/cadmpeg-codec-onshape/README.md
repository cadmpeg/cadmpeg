# cadmpeg-codec-onshape

Read `.onshape-bundle` version 1 into CADIR v11. The registry supplies the
STEP decoder. Both decoders share one decode context.

<!-- generated: capability onshape -->

Support: L0 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#onshape-bundle)).

<!-- /generated: capability onshape -->

The [bundle contract](../../docs/formats/onshape.md) defines the manifest,
canonical paths, consistency rules, units and bounds. Admitted nonempty member bytes
are embedded as assets. Empty members carry `container.empty-member`. Unsupported history carries stable `onshape/*`
loss codes.

Tests build ZIP members from synthetic response shapes in Rust. The six
builders cover plate/fillet, revolve, holes, hole forms, unsupported history,
and compressed default-plane regions. Their response data comes from the
reader's code-authored generators. The STEP member is built from a synthetic
CADIR solid with the cadmpeg STEP encoder. ZIP timestamps use the writer's
fixed default. Tests require an offline process.

A present thread pitch that cannot be admitted carries `onshape/record.malformed`.
The hole keeps its thread specification without pitch.

Admitted thin walls use typed design operations and Native model companions.
The design timeline carries the decoded sketches, their placed frames and
solved curves. Thin profile picks name those design sketch entities. Model
sketches, constraints and parameters retain their decoded constructions.
`onshape/extrude.thin-model-not-supported` names the model construction gap.
The scoped design timeline includes every admitted model feature. Native design members
carry `onshape/history.design-operation-native`; the model rows retain their
decoded constructions. Final STEP bodies bind after the complete timeline.
Failed thin sketch projection or scope installation restores the design state
and reports `onshape/extrude.thin-not-supported`. The bundle retains its native
model operations. Session resource limits propagate to the caller. Feature
annotations use the first admitted source record for each model feature.
Explicit historical provenance passes the stored rollback producer proof,
including tangent-added edges.

Unreadable members, invalid names, encryption, unsupported compression and repeated
names carry `onshape/member.unreadable`. The first member with each name is
admitted. JSON responses with stale or malformed microversion declarations
are omitted with the same loss. Malformed feature arrays carry
`onshape/history.not-supported` and retain readable final geometry.

Member size, remaining bundle capacity and expansion limits omit the affected member with
`onshape/member.too-large`. Topology row limits omit the affected rollback
table with `onshape/topology.too-large`; dependent features use Native
construction and their mapping loss. Unreadable STEP exports retain readable
history with `onshape/step.unreadable` and missing final geometry. Session
expansion, retained-byte and entity budgets remain decode errors. A bundle
with empty history and unreadable final geometry remains a decode error.
Each model decode states one root product definition, including a decode
that retains history after a STEP member failure.

Rollback edges with unsupported curves or incomplete geometric fields retain their
ids and carry `onshape/topology.edge-geometry-unsampled`. The loss names the edge
and the sampling failure. Stored-value proofs that need its geometry refuse.

The design timeline states the stored rollback bar. An absent or invalid
`rollbackIndex` states the bar after the last feature and carries
`onshape/history.rollback-unread`.
