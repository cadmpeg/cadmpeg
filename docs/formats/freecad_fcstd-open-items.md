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

**Note.** The `Angle3Pt` custom label already identifies the three-point category.

## TD-03. Arc sweep and center-line adjustment

**Question.** How do the cosmetic arc sweep fields and the center-line shift, rotation, extension,
and flip fields give the stored curve?

**Known.** Arc records contain a center, a radius, end points, angles, `Clockwise`, and `Large`.
Center-line records contain branch geometry and the `HShift`, `VShift`, `Rotate`, `Extend`, and
`Flip` fields.

**Need.** Nonempty arc and center-line records whose sweep and adjustment values are set
independently.
