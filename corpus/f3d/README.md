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
