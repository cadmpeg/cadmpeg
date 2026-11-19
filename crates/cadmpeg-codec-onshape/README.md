# cadmpeg-codec-onshape

Read `.onshape-bundle` version 1 into CADIR v11. The registry supplies the
STEP decoder. Both decoders share one decode context.

<!-- generated: capability onshape -->

Support: L0 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#onshape-bundle)).

<!-- /generated: capability onshape -->

The [bundle contract](../../docs/formats/onshape.md) defines the manifest,
canonical paths, consistency rules, units and bounds. Nonempty member bytes
are embedded as assets. Empty members carry `container.empty-member`. Unsupported history carries stable `onshape/*`
loss codes.

Tests build ZIP members from synthetic response shapes in Rust. The six
builders cover plate/fillet, revolve, holes, hole forms, unsupported history,
and compressed default-plane regions. Their response data comes from the
reader's code-authored generators. The STEP member is built from a synthetic
CADIR solid with the cadmpeg STEP encoder. ZIP timestamps use the writer's
fixed default. Tests require an offline process.

Admitted thin walls use typed design operations and Native model companions.
`onshape/extrude.thin-model-not-supported` names the model construction gap.
The scoped design timeline includes every source feature. Native design members
carry `onshape/history.design-operation-native`; the model rows retain their
decoded constructions. Final STEP bodies bind after the complete timeline.
Explicit historical provenance passes the stored rollback producer proof,
including tangent-added edges.
