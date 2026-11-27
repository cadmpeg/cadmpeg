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

Each Part Studio states its v11 design layer. The scope is the studio's
root definition, and one ordered timeline holds a design feature for each
model feature, in `features.json` order, with the stored rollback bar. A
design feature has the id of its model feature. Sketches, solid extrudes of a
whole sketch (blind, through all, symmetric, up to next), thin extrudes,
revolves, and offset datum planes take their typed operation. Every other
feature takes its operation family; where the model row decodes more than the
family, `onshape/history.design-operation-unresolved` names the feature. A
derived Part Studio is a native design operation, and
`onshape/history.design-operation-native` names its captured design, which
stays in the model row. A sketch whose frame or curves the
design cannot state falls back to its family alone. Variables and feature and
sketch dimensions are design parameters with neutral expressions; an
extrude's depth and a revolve's angle bind them. Sketch constraints are
design relations, on point entities for the curve ends and centres they name.
A thin extrude keeps a Native model operation, and
`onshape/extrude.thin-model-not-supported` names the gap; a thin design
feature that the projection refuses keeps its family and reports
`onshape/extrude.thin-not-supported`. The root Assembly states its scope: a
Joint feature for each mate, and a grounded entry for each fixed occurrence.
Session resource limits propagate to the caller. Feature annotations use the
first admitted source record for each model feature. Explicit historical
provenance passes the stored rollback producer proof, including tangent-added
edges.

Unreadable members, invalid names, encryption, unsupported compression and repeated
names carry `onshape/member.unreadable`. The first member with each name is
admitted. JSON responses with stale or malformed microversion declarations
are omitted with the same loss. `sketches.json` retains its member bytes when
its JSON or microversion is unreadable. Malformed feature arrays carry
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

Unsupported construction plane forms carry `onshape/plane.not-supported`.
Unresolved plane quantities and supports carry `onshape/history.not-supported`.
A mapped sketch on a created datum plane with an unresolved attachment matrix
uses the support frame and carries `onshape/sketch.plane-assumed`.
