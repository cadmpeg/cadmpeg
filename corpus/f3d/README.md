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

Compute option and target bodies. Written in Autodesk Fusion 2706.1.17. A
40 × 40 × 5 mm plate with a 4 mm hole cut at (10, 20) mm, patterned along X
with Compute Optimized unless stated.

- `optimized_extent_pattern.f3d`: quantity 3, Extent 20 mm. The copies are at
  0, 10 and 20 mm. 7811.50 mm³, 9 faces.
- `optimized_symmetric_pattern.f3d`: quantity 3 at 10 mm, Symmetric. The copies
  are at −10, 0 and +10 mm; the −10 mm copy cuts the plate edge. 7842.92 mm³,
  10 faces.
- `two_target_pattern.f3d`: two plates at x 0–40 and 41–81 mm and one 21 × 4 mm
  slot cut through both, patterned along Y with quantity 3 at 10 mm, Compute
  Adjust. 2 bodies, 14800 mm³, 36 faces.

# Fusion API fixtures: Combine tools

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2704.1.53 from a new design. A target box (Extrude 1) is joined with tool boxes
that each come from their own Extrude and partly overlap it. Keep Tools is off.
The API reported the tool count, and the body count, volume and face count after
the Combine.

- `combine_tools_n1.f3d`: one tool. 1 body, 9480 mm³, 12 faces.
- `combine_tools_n2.f3d`: two tools. 1 body, 9960 mm³, 18 faces.
- `combine_tools_n3.f3d`: three tools. 1 body, 10440 mm³, 24 faces.

# Fusion API fixtures: surface revolves

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2706.1.17 from a new design. A Sketch on the XZ origin plane is revolved about
the Z origin axis as a new surface body (isSolid false), with an angle extent of
90 deg and Symmetric on. The API reported the body count, area and face count
after the Revolve.

- `symmetric_surface_revolve.f3d`: a closed rectangle at x 5–10 mm, z 0–10 mm.
  1 surface body, 706.85835 mm², 4 faces: cylinders at radius 5 and 10 mm and
  two planar annular faces.
- `open_profile_surface_revolve.f3d`: one open line from (10, 0, 0) to
  (10, 0, 10) mm, selected through createOpenProfile. 1 surface body,
  314.15927 mm², 1 cylindrical face.

# Fusion API fixtures: Copy/Paste Bodies

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2706.1.17 from a new design. Each pastes boxes of 10 × 10 mm (5, 6, 7 or 8 mm
tall) into a new component `Target`, whose occurrence is at y + 30 mm, with one
Copy/Paste Bodies operation. The API reported the body count, volume and face
count after the operation.

- `copy_paste_one_body.f3d`: one 5 mm box. 2 bodies, 1000 mm³, 12 faces.
- `copy_paste_two_bodies.f3d`: two boxes of one component. 4 bodies, 2600 mm³,
  24 faces.
- `copy_paste_two_components.f3d`: one box from each of two components.
  4 bodies, 2600 mm³, 24 faces.
- `copy_paste_three_bodies.f3d`: three boxes of one component. 6 bodies,
  3600 mm³, 36 faces.

# Fusion API fixtures: Reverse Normal

These files are CC0. Each was written through the Fusion API in Autodesk Fusion
2706.1.17 from a new design. Each Sketch on the XZ origin plane holds a closed
rectangle, revolved about the Z origin axis as a new surface body (isSolid
false), with an angle extent of 90 deg and Symmetric on. A Reverse Normal of
the surface bodies follows. The API reported the timeline, the body count and
face count after each feature, and the bodies that the Reverse Normal selects.

- `reverse_normal_revolve.f3d`: a rectangle at x 5–10 mm, z 0–10 mm, and a
  Reverse Normal of its one body. 1 surface body, 4 faces.
- `reverse_normal_two_revolves.f3d`: rectangles at x 5–10 mm and x 15–20 mm,
  z 0–10 mm, each revolved by its own Revolve, and one Reverse Normal of both
  bodies. 2 surface bodies, 8 faces.

# Fusion fixtures: Reverse Normal of several bodies and components

These files are CC0, written in Autodesk Fusion 2706.1.17 from new designs.
Each surface body is a 20 mm line extruded 10 mm as a surface.

- `reverse_normal_two_bodies.f3d`: API-built. One Reverse Normal of two surface
  bodies of one component. 2 bodies, 2 faces.
- `reverse_normal_two_components.f3d`: the API built two components, each with
  one surface body, the second occurrence at y + 30 mm; a person made one
  Reverse Normal in the root that selects both surfaces in the Fusion UI.
  Fusion stores it as `Reverse Normal1` in each component, linked, as one
  timeline item.

Reverse Normal across components, made in the Fusion UI with the root component
active: the surfaces preselected, then Surface > Reverse Normal. Each is one
timeline item, `Reverse Normal1`, and the API reports one linked Reverse Normal in
each component. Every normal turns from (0, −1, 0) to (0, +1, 0).

- `reverse_normal_three_components.f3d`: one surface body in each of the
  components C1, C2 and C3, at y = 0, 30 and 60 mm.
- `reverse_normal_two_plus_one.f3d`: two surface bodies in C1 and one in C2.
