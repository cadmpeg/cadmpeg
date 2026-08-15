# Rhino openNURBS comparison

The comparison has a source-file tier and two synthesized tiers. The source
tier uses the 153 `.3dm` files in the openNURBS `example_files` directory. It
runs `example_read` for every file, decodes the same file with the Rhino codec,
validates each decoded CADIR, and checks the source object total for every
archive version. The synthesized tiers use a separate writer for one point and
for six structured objects: a point, line, arc, polyline, quad mesh, and box
Brep. The external reader and the Rhino codec must both accept every
synthesized archive.

Run the comparison from the repository root:

```text
python3 tools/validate_rhino_opennurbs.py /path/to/opennurbs
```

The committed transfer floors are:

| Archive | Supported floor | Source objects |
| ------: | --------------: | -------------: |
|       2 |           1,989 |          2,342 |
|       3 |           2,413 |          2,477 |
|       4 |              47 |            173 |
|      50 |              92 |            198 |
|      60 |              28 |             37 |
|      70 |              31 |             46 |
|      80 |              24 |             39 |

The supported count is a per-version minimum. It is not a monotonicity claim
and it is not complete object coverage. The test fails when `example_read`
refuses a file, validation reports an error, a source-object total changes, or
a supported-object count falls below its floor.

Archive version 1 is a reader-only L0 boundary. Archive version 5 has header
inspection only. The transfer claim therefore covers archive versions 2, 3, 4,
50, 60, 70, and 80 with the floors above. Native source-less writing targets
50, 60, 70, and 80. For each writing target, the test writes one point with
`RhinoEncoder`, requires `example_read` to enumerate one model object, and
decodes the same file as one transferred object. This checks both directions
at the object-record boundary; it does not claim acceptance by a vendor UI.

The byte-level transfer boundary is class-specific:

| Class family | Admission discriminator |
| ------------ | ----------------------- |
| Every registered class | The class-data body can interleave direct fields and complete child chunks. The class grammar owns each nested boundary; the class UUID alone does not make the body a flat chunk sequence. |
| `ON_Brep` and registered Brep aliases | C2, C3, and surface arrays are positional. Empty vertex edge lists are valid. Singular and point-on-surface trims use edge `-1` and identical endpoints. A free vertex has no serialized shell field, so it is assigned only when one shell owns it; ambiguous shell ownership retains the object atomically. |
| `ON_PlaneSurface` and surface carriers | U/V domains and U/V extents are separate intervals. Parameter curves use the affine domain-to-extent map before the plane frame is evaluated. |
| `ON_Extrusion` | The profile is a polymorphic curve. Profile count, closure, outer-then-inner orientation, path interval, cap state, and miter gates control admission; a single primitive profile is not the general wire form. |
| V2–V4 text and leader classes | Their class data starts with packed version `1.0` and the direct common fields. The later anonymous outer wrapper is absent. |
| `ON_Mesh`, NURBS curves, and NURBS surfaces | Version gates, declared counts, finite values, channel sizes, knot/CV counts, and domains must agree. A bounded suffix is not reinterpreted as another child record. |

Unknown classes and records that fail these invariants remain complete opaque
records. They are not counted as transferred geometry, and the loss is charged
at the object boundary.
