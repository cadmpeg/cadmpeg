# IronCAD scene files

These files are CC0 donations authored in IronCAD 2025 (27.0 Product Update 1
Service Pack 2, 64-bit build 19811). Each file starts from a new, empty scene.
The donation rules in [`../README.md`](../README.md) apply.

Unless a file states otherwise, the scene units are millimetres, degrees, and
kilograms, every part has no material, the density 7800 kg/m³, and the default
colour, and its anchor lies at the scene origin with no rotation. Volume, area,
and mass are the values that IronCAD states for the part.

| File | Part | Kernel | Volume (mm³) | Area (mm²) | Mass (kg) |
|---|---|---|---|---|---|
| `Box_50x40x25.ics` | 50 x 40 x 25 mm block | Parasolid | 50000 | 8500 | 0.390 |
| `Cylinder_50x50.ics` | cylinder, diameter 50 mm, height 50 mm | Parasolid | 98174.770 | 11780.972 | 0.766 |
| `Block_ACIS_100x50x20.ics` | 100 x 50 x 20 mm block | ACIS | 100000 | 16000 | 0.780 |
| `Block_Parasolid_100x50x20.ics` | 100 x 50 x 20 mm block | Parasolid | 100000 | 16000 | 0.780 |

The blocks are catalog shapes sized by their handles. IronCAD shows 2 edges and
no vertices on the cylinder; its stored body adds a seam edge with two
vertices.

Placement and units:

- `Block_moved.ics`: the 100 x 50 x 20 mm Parasolid block, moved by the TriBall
  to (30, -20, 10) mm.
- `Block_rotated.ics`: the same block, turned by the TriBall 30 degrees about Z
  at the scene origin.
- `Assembly_moved.ics`: two 50 x 40 x 25 mm Parasolid blocks in one assembly,
  the second 25 mm above the first. The assembly, and not its parts, is moved to
  (30, -20, 10) mm.
- `Block_inch.ics`: one 4 x 2 x 1 in Parasolid block in a scene set to inches,
  degrees, and kilograms.

Mixed scene:

- `Ironcad_2025_simple_test_parts.ics`: the parts `Box_Parasolid`,
  `Cylinder_ACIS` (ACIS kernel), `Compound_part` (a block and a cylinder with a
  through hole and a blend), `Part4`, and `Part6`, made with IntelliShapes and
  sized by their handles. `Assembly` holds `Part4` and `Part6`; the other parts
  are parts of the scene.

STEP exports:

- `Ironcad_2025_simple_test_parts.stp` and `Assembly_moved.stp`: AP214 exports
  of the scenes of the same name, written by IronCAD. They state the product
  tree, the part placements, and the body geometry of each scene. In
  `Assembly_moved.stp` the assembly is the root product, and its offset is part
  of each part placement.

Parasolid export:

- `Cylinder_50x50.x_t`: a Parasolid 34.0 text transmit export of the
  cylinder of `Cylinder_50x50.ics`. Its body has 3 faces, 3 edges, and 2
  vertices: the two rim circles and a straight seam edge between the vertices
  at (0.025, 0, 0) and (0.025, 0, 0.05) m. The header values `USER`, `KEY`, and
  `FILE` are replaced by `unknown`. No codec reads this format, so the file has
  no manifest entry.

## Requested files

Assembly structure:

- `assembly_between_parts.ics`: one part, then an assembly of two parts, then a
  second part added after the assembly.
- `assembly_nested.ics`: that assembly placed inside a second assembly.

Export each requested scene to STEP as well.

History:

- `block_then_block.ics`: one part with one block feature, then a second block
  feature added.
- `extrude_sketch.ics`: one extrude from a 2D profile.

Geometry carriers:

- `block_fillet.ics`: the block with one edge filleted, radius 5 mm.
- `sphere.ics`: one sphere shape, diameter 50 mm.
