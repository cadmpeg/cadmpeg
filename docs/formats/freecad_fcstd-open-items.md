# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

When an item is resolved, delete it in the same change that writes the answer into
the specification. Do not keep a Resolved part. Neutral projection limits are
listed in [the coverage document](freecad_fcstd-coverage.md).

## TD-01. Three-point angle selector without custom labels

**Question.** Which `Type` selector identifies a three-point angle without a `CustomEnumList`?

**Known.** Without custom labels, selectors 0, 1, 2, 4, 5, and 6 identify distance,
horizontal distance, vertical distance, radius, diameter, and angle. A
`CustomEnumList` supplies ordered category labels indexed by the selector and
has precedence over these defaults.

**Need.** A stored three-point angle without a `CustomEnumList` and its selected `Integer`.

**Conflict.** None.

**Note.** The `Angle3Pt` custom label already identifies the three-point category. That labeled case is settled; only the stored selector without a `CustomEnumList` remains open.

## TD-03. Arc sweep and center-line adjustment

**Question.** How do the cosmetic arc sweep fields and the center-line shift, rotation, extension,
and flip fields give the stored curve?

**Known.** Arc records contain a center, a radius, end points, angles, `Clockwise`, and `Large`.
Center-line records contain branch geometry and the `HShift`, `VShift`, `Rotate`, `Extend`, and
`Flip` fields.

**Need.** Nonempty arc and center-line records whose sweep and adjustment values are set
independently.

## TD-04. Sketch profile chaining under the endpoint work cap

**Question.** What endpoint-matching rule chains sketch entities into profiles when candidate pairs reach the decoder work cap?

**Known.** `freecad_fcstd.md` §8 "Sketch persistence has separate ordered" states that the format has no profile chain, profile seed entity, endpoint-junction tolerance, or junction tie-break rule. The decoder builds profiles with a work budget of 16,777,216 endpoint candidates per sketch. When matching exhausts the budget, the decoder emits `SketchProfileMatchingBounded` and transfers no profiles while retaining the sketch entities and constraints.

**Need.** A source rule or a specimen with dense endpoint candidates near the cap must settle the chaining order, the junction tolerance, and the tie-break rule. Until then the neutral profile set is an implementation-capped selection.

**Note.** QA sweep location: `crates/cadmpeg-codec-freecad/src/design.rs:1909-1918`. This is the FreeCAD analogue of the unrecorded selection threshold in ASM GC-34.

## TD-05. Mesh placement expansion under the work allowance

**Question.** How many mesh placements does a native reader expand, and in which order, when occurrence uses exceed the placement work allowance?

**Known.** `freecad_fcstd.md` §7 "Polygon carriers are also transferred as bounded neutral geometry" gives the bounded neutral transfer of stored polygons and triangulations. The decoder expands occurrence placements with a work allowance of 64 per record plus occurrence, floored at 65,536. When expansion exhausts the allowance, the decoder truncates the output, withholds that occurrence and later uses, and emits `MeshPlacementBounded`.

**Need.** A source rule or a specimen with placement uses near the allowance must settle the expansion order and the truncation point. Until then the neutral tessellation set is an implementation-capped selection.

**Note.** QA sweep location: `crates/cadmpeg-codec-freecad/src/application_geometry/mesh_placement.rs:191-204`. This is the FreeCAD analogue of the unrecorded selection threshold in ASM GC-34.

## TD-06. Location identity expansion under the term and work caps

**Question.** What native location-identity rule applies when a shape payload exceeds 65,536 location terms or the 16,777,216-unit composition work bound?

**Known.** `freecad_fcstd.md` §7 "Location zero is the empty datum chain" gives the reduced-chain identity of composed elementary locations. The decoder caps a location identity at 65,536 terms and composition work at 16,777,216 units. When expansion reaches either cap, the decoder emits `TopologyLocationIdentityBounded` and retains the payload natively without topology transfer.

**Need.** A source rule or a specimen with deep location chains near either cap must settle the reduction order and the truncation point. Until then the neutral occurrence set is an implementation-capped selection.

**Note.** QA sweep locations: `crates/cadmpeg-codec-freecad/src/brep/location_identity.rs:57-90` and `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:136-148`. This is the FreeCAD analogue of the unrecorded selection threshold in ASM GC-34.
