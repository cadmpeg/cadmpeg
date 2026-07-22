# IR pre-alignment (v54) — branch integration guide

Temporary integration guidance for the in-flight `cadmpeg-*` worktree branches.
Delete this file once every branch has landed.

IR v54 settles the schema shapes that two or more branches invented
independently. Each branch merges `origin/main` (never rebase), applies its
adaptations below, gets CI green on the branch, then squash-merges to `main`.
Each branch that changes the schema takes the next free `IR_VERSION` at landing
time and rewrites `PREVIOUS_IR_VERSION` plus the single-step migration
accordingly.

## Settled decisions

- `LossCategory::DesignIntent` is the loss category for features, sketches,
  parameters, configurations, and design history. There is no
  `LossCategory::Feature`.
- `FeatureDefinition::DatumPlaneUnresolved` exists once, placed immediately
  after `DatumPlane`. Unresolved datum variants sit immediately after their
  resolved counterpart.
- `FeatureDefinition::SpatialSketch { sketch: Option<SpatialSketchId> }` is the
  history node for spatial sketches.
- The spatial-sketch core (`SpatialSketch`, `SpatialSketchProfile`,
  `SpatialSketchEntityUse`, `SpatialSketchEntity`, `SpatialSketchGeometry`,
  arenas `spatial_sketches` and `spatial_sketch_entities`) uses the
  profile-bearing shape: sketches own ordered `profiles`, each with a
  model-space plane and oriented boundary uses. There is no flat ordered
  `entities` list on `SpatialSketch`; entity ordering is arena order.
- `PcurveGeometry::Circle` and `Ellipse` keep the `x_axis`/`y_axis` form
  already on `main`. Do not reintroduce `ref_direction`/`clockwise` variants;
  a clockwise parameterization is a negated `y_axis`.
- `SketchSpace` is scheduled for deletion: `Sketch { space: Spatial }` becomes
  `FeatureDefinition::SpatialSketch`. The first of sldprt/f3d to land removes
  the enum, the `space` field, and ships the real JSON migration for it.
- Version literals in tests and generators reference
  `cadmpeg_ir::IR_VERSION`/`PREVIOUS_IR_VERSION`, never a hardcoded number.

## Per-branch adaptations after merging `origin/main`

- **catia**: drop its own `LossCategory::DesignIntent` addition if the merge
  duplicates it (same name, same anchor — expected to merge clean). No IR bump.
- **creo**: drop its duplicate `DatumPlaneUnresolved` (it anchored the variant
  before `DatumPlane`; main now has it after). Re-take `IR_VERSION` as the next
  free number with `migrate_previous_sketch_placements` as the single-step
  migration.
- **nx**: rename `LossCategory::Feature` call sites to `DesignIntent`. Its
  `DatumPlaneUnresolved` matches main's anchor and should merge clean;
  `DatumPointUnresolved` is nx-only and lands with nx. Re-take `IR_VERSION`
  as the next free number (pure restamp).
- **platform**: no schema collisions; integrates on merge order alone.
- **sldprt**: re-derive against main's spatial-sketch core: no `entities`
  field, no `SpatialSketchGeometry::Circle/Arc` without `reference_direction`
  (adopt the `reference_direction` field shapes), keep `PolarHarmonic`/
  `PolarNurbs` as its own additive pcurve variants, rewrite `Circle`/`Ellipse`
  pcurve emission to the `x_axis`/`y_axis` form. Owns the `SketchSpace`
  deletion if it lands before f3d.
- **f3d**: re-derive its IR delta on top of v54+; the spatial-sketch core is
  already on main in f3d's shape, so its remaining IR delta is
  `SpatialSketchConstraint` (additive, with the third arena) and its
  feature-definition reshapes, each justified on its own.
  `SpatialSketchGeometry::NurbsSurface` is already on main awaiting its
  producer. Owns the `SketchSpace` deletion if it
  lands before sldprt.
