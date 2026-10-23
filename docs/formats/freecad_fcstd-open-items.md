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

## 2. Container identity

### WR-01. Deferred side-entry restoration order

**Question.** What order must ZIP members follow so document, GUI, string-table,
shape, and property-owned payloads restore with their dependencies across the
supported document schemas?

**Known.** `freecad_fcstd.md` §1 "The synthesis catalog is"
requires `Document.xml` first and preservation of named entries. The writer then
sorts the other entries by name. A native-authored two-box document can lose one
saved shape on native reopen after this sort, although the XML and uncompressed
side entries survive. A document with two `Part::Feature` boxes declared as
`ZFirst`, then `ASecond`, reopens with `ZFirst.Shape` null after an FCStd rewrite.
Each original box has six faces and volume 6,000 mm³. The conversion reports no
findings or losses. Preserving source member order avoids the missing shape.

**Need.** Define a deterministic restoration schedule for preserving writes and
source-less construction, including shared payload dependencies and GUI entries.
The safe order for one retained archive does not define that schedule for every
supported graph or schema.

**Conflict.** The current write checks treat preservation by entry name and digest
as semantic preservation. Native restoration can change when member order changes.

**Note.** The member sort is in `crates/cadmpeg-codec-freecad/src/writer.rs`.
Check per-object shape nullness, topology counts, volume, and placement on native
reopen after an unchanged write and a typed leaf edit. Entry name and digest
equality does not supply those checks.

## 7. Exact shapes

### WR-02. STEP replica transformation attributes

**Question.** Which inherited attributes and parameter order must a 2D replica
transformation carry for FCStd-to-STEP output to satisfy its declared STEP schema
and native import contract?

**Known.** `crates/cadmpeg-codec-step/src/geometry.rs` emits five parameters for
`CARTESIAN_TRANSFORMATION_OPERATOR_2D`. The reader's `transformation_parameter`
in `crates/cadmpeg-codec-step/src/reader/geometry.rs` accepts five- and
seven-parameter forms. A 10 by 20 by 30 mm box with a 1 mm fillet on all edges
passes its FCStd check, STEP AP214 conversion, and cadmpeg's STEP check. Native
STEP import in FreeCAD 1.0.0 with OCCT 7.8.1 crashes during 2D pcurve replica
transformation construction. Adding two leading empty string attributes to each
of the 16 affected operators avoids that crash and yields a valid 26-face shell.

**Need.** State the schema-specific inherited attributes and emit the required
record. Validate its arity independently of the reader's permissive dispatch.
Exercise reflected conic pcurves that still require replica operators, and check
native geometry after import.

**Conflict.** Agreement between the emitter and cadmpeg's reader permits an
output that the native importer cannot consume. Other export losses name
degenerate-edge omission and native metadata, but do not identify this operator
incompatibility.

**Note.** This is a remaining interoperability case after
[PR #284](https://github.com/cadmpeg/cadmpeg/pull/284). The seven-parameter control
isolates the crash; it is not a complete production fix. Solid classification
in [PR #319](https://github.com/cadmpeg/cadmpeg/pull/319) and trimmed-curve parameter
transfer in [PR #323](https://github.com/cadmpeg/cadmpeg/pull/323) are separate
defects. The analogous 3D emitter needs review; the case above confirms only 2D.

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
