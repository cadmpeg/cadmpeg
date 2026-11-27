# IronCAD scene files

These files are CC0 donations authored in IronCAD 2025. Each file starts
from a new, empty scene. The donation rules in [`../README.md`](../README.md)
apply.

- `Block_ACIS_100x50x20.ics`: one 100 x 50 x 20 mm block part, ACIS kernel.
- `Block_Parasolid_100x50x20.ics`: the same block, Parasolid kernel.
- `Box_50x40x25.ics`: one 50 x 40 x 25 mm block part, Parasolid kernel.
- `Cylinder_50x50.ics`: one cylinder part, diameter 50 mm, height 50 mm,
  Parasolid kernel.
- `Ironcad_2025_simple_test_parts.ics`: the parts `Box_Parasolid`,
  `Cylinder_ACIS` (ACIS kernel), `Compound_part` (a block and a cylinder with a
  through hole and a blend), `Part4`, `Part6`, and one assembly.

## Requested files

Placement:

- `block_moved.ics`: the 100 x 50 x 20 mm block translated by (20, 30, 50) mm.
- `block_rotated.ics`: the block rotated 30 degrees about the Z axis.

History:

- `block_then_block.ics`: one part with one block feature, then a second block
  feature added.
- `extrude_sketch.ics`: one extrude from a 2D profile.

Geometry carriers:

- `block_fillet.ics`: the block with one edge filleted, radius 5 mm.
- `sphere.ics`: one sphere shape, diameter 50 mm.

Units:

- `block_inch.ics`: the block with the scene units set to inches.
