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

## TD-01. Dimension `Type` selectors other than 0

**Question.** Which dimension category does each `Type` selector other than 0 identify when the
dimension has no `CustomEnumList`?

**Known.** `Type` contains an `Integer` selector. Selector 0 identifies a distance. A
`CustomEnumList` supplies ordered category labels indexed by the selector.

**Need.** Saved horizontal, vertical, radius, diameter, angle, and three-point angle dimensions
without a `CustomEnumList`, with their serialized selectors.

## TD-02. Measurement from nonempty `SavedGeometry`

**Question.** How does a nonempty `SavedGeometry` shape list give the dimension measurement?

**Known.** `SavedGeometry` names ordered BREP members. An empty shape with `TShapes 0` holds no
measurement.

**Need.** Nonempty saved shapes paired with a known dimension category, its reference geometry,
and its displayed value.

## TD-03. Arc sweep and center-line adjustment

**Question.** How do the cosmetic arc sweep fields and the center-line shift, rotation, extension,
and flip fields give the stored curve?

**Known.** Arc records contain a center, a radius, end points, angles, `Clockwise`, and `Large`.
Center-line records contain branch geometry and the `HShift`, `VShift`, `Rotate`, `Extend`, and
`Flip` fields.

**Need.** Nonempty arc and center-line records whose sweep and adjustment values are set
independently.
