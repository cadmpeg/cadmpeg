# F30 inserted-part placements

These files are CC0 CAD Fixture Recipes files, authored in Fusion 2706.1.17.
The names contain the recipe id.

F30a: Owner B is inside Owner A. Each owner contains one insertion of the
shared L-part. The L-part's local box is (0, 0, 0)..(40, 20, 10) mm, with
volume 5000 mm³. Both owners are ungrounded. The file is saved without
Capture Position.

F30b: F30a with Owner A translated by (20, 30, 50) mm and Capture Position
selected.

# Fusion API fixtures: profile regions

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2704.1.53 from a new design, for the nested profile-region selection. The API
reported the volume, body count and face count after each timeline item; those
values are below.

Profile regions. Each file extrudes one nested profile-region selection.

- `region_A.f3d`: one selected region that is one face of the sketch
  arrangement. 40.0 mm³.
- `region_B.f3d`: one selected region of two arrangement faces. 80.0 mm³.
- `region_C.f3d`: one selected region with a hole. 543.4513 mm³.
- `region_D.f3d`: two separate selected regions. 40.0 mm³.
- `strip_cut.f3d`: a cut whose one Sketch-profile operand selects three of five
  profiles (3.0, 14.61354 and 2.0 mm²), then a rectangular pattern of the cut.
  Extrude1 990 mm³, 6 faces; the cut 975.38646 mm³, 10 faces; the pattern
  898.882541 mm³, 30 faces.

# Fusion API fixtures: removed bodies

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2704.1.53 from a new design, for body selections of a removed body. The API
reported the volume, body count and face count after each timeline item; those
values are below.

Removed bodies.

- `mirror_remove.f3d`: Extrude1 1000 mm³, 6 faces; a cut, 993.716815 mm³,
  8 faces; Mirror1 of the body across the YZ origin plane, 2 bodies,
  1987.43363 mm³, 16 faces; RemoveBody of the original body, 1 body,
  993.716815 mm³, 8 faces.

# Fusion API fixtures: rectangular patterns

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2704.1.53 from a new design, for rectangular-pattern decoding. The API reported
the volume, body count and face count after each timeline item; those values are
below.

Rectangular-pattern Compute option. One cut that ends on a slanted body edge,
patterned five times at 2 mm. The API added a second direction of count 3 and
zero distance.

- `compute_adjust.f3d`: Compute Adjust.
- `compute_optimized.f3d`: Compute Optimized; removes 0.1414 mm³ more than Adjust.
- `compute_identical.f3d`: Compute Identical; Fusion reports the pattern feature
  failed.

Symmetric patterns. Quantity 4 at 5 mm with Symmetric on; direction two has
quantity 1 and keeps its 5 mm distance. Fusion places the copies at
x = −5, 0, +5 and +10 mm.

- `symmetric_pattern.f3d`: a cut seed. Extrude2 2394 mm³, 11 faces; the pattern
  2376 mm³, 26 faces.
- `symmetric_pattern_body.f3d`: a body seed (a 1 × 6 × 1 mm block), Compute
  Adjust. Extrude2 2406 mm³ in 2 bodies, 12 faces; the pattern 2424 mm³ in 5
  bodies, 30 faces.
