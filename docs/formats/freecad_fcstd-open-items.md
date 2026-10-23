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



## 11. Presentation and application records

### AG-01. Mesh placement in neutral geometry

**Question.** How must an application mesh's local coordinates and owning
`Placement` compose with parent containers and link occurrences in neutral geometry?

**Known.** `parse_mesh` in
`crates/cadmpeg-codec-freecad/src/application_geometry.rs` copies the binary
vertices into the neutral tessellation without the owning object's `Placement`.
A `Mesh::Feature` triangle with local vertices `(0,0,0)`, `(2,0,0)`, `(0,3,0)`,
translation `(100,200,300)` mm, and a 90-degree Z rotation has native world
coordinates `(100,200,300)`, `(100,202,300)`, `(97,200,300)`. The neutral mesh
keeps the local coordinates. No neutral body, occurrence, or product definition
carries the missing transform. The source check reports no findings or losses.

**Need.** Define and implement the placement composition so each transform
applies once. Check translation, rotation, nested containers, linked meshes, and
mesh arrays against native world coordinates. Name any unsupported placement
path in a loss rather than returning geometry in the wrong frame.

**Conflict.** The mesh is reported as transferred, but its neutral position and
orientation differ from the native object. Retaining the native placement
property does not correct the neutral tessellation consumed by other formats.

**Note.** Keep mesh placement separate from point-cloud transform handling and
from appearance. A point-cloud control with the same placement produces the
expected world coordinates.

## 12. Validation

### QA-01. Native geometry equivalence and profile assertions

**Question.** Which independent geometry and design invariants must the native
acceptance helper and generated profile verify before they report a successful
semantic write?

**Known.** `tools/validate_fcstd_interop.py` accepts a document returned by
`openDocument`. Its optional recompute/save/reopen path compares object names
and types, without checking the recompute result, object error states, or
geometry. With `CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE=1`, the WR-01 two-box output
reports `accepted=true`, `native_resave_accepted=true`, and two objects although
one shape is null.

The profile in `crates/cadmpeg-codec-freecad/src/bin/fcstd_profile.rs` checks
required topology kind names at L3, a nonzero appearance binding count at L5,
and required constraint and operation labels at L6. Its L9 assertions compare
cadmpeg fingerprints, a document-label edit, and entry name/digest equality.
Neutral and native validation also run, but these checks do not establish every
independent semantic guarantee named in the coverage matrix.

**Need.** Compare native results with the original or an authored oracle for
shape nullness, solid and face counts, volume, placement, recompute results,
and error states. Check appearance ownership and precedence across the required
target classes, and compare the recomputed design census and design edits.
Align each profile assertion with the guarantee that its coverage row names.

**Conflict.** The helper certifies the missing-shape output. Self-round-trip
and record-presence assertions can pass while native geometry or a required
semantic comparison is absent.

**Note.** Add the missing-shape case to the acceptance criteria of
[issue #243](https://github.com/cadmpeg/cadmpeg/issues/243). Keep document-open,
self-round-trip, and native semantic-equivalence results distinct. The published
headline remains L5; the L9 write extras do not establish full application support.
