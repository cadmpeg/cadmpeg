# Measurement fixtures

All fixtures in this directory are original minimal exchange structures
authored against `docs/formats/step.md` and `docs/formats/iges.md`. Each one states its geometry
exactly, so the measurement tests derive every expected value from a
closed-form expression.

| File | Construct |
| --- | --- |
| `measure_cylinder_solid.p21` | Closed solid right circular cylinder: axis +z through the origin, radius 10, z from 0 to 30. One cylindrical side face with one seam line edge, two planar disc faces, two circle edges. |
| `measure_cylinder_patch.p21` | Open shell with one cylindrical face: the patch of the cylinder of radius 10 about the z axis with `u` from 0 to π/2 and z from 0 to 30. Two quarter-circle arcs and two line edges. |
| `measure_plane_hole.p21` | Open shell with one planar face: the square from (0,0,0) to (40,40,0) with a hole of diameter 20 centred at (20,20,0). One outer ring of four line edges and one inner ring of one circle edge. |
| `measure_sphere_cap.p21` | Open shell with one spherical face: the cap of the sphere of radius 10 about the origin above z = 6, bounded by one circle edge of radius 8 at z = 6 that runs counterclockwise about +z. The face sense is forward. |
| `measure_implicit_outer_bspline.igs` | Type 144 trimmed surface with outer flag 0 on the bilinear B-spline patch from (0,0,0) to (1,1,0), with one inner boundary: the square from (0.25,0.25) to (0.75,0.75) in model and parameter space. |
| `measure_implicit_outer_plane.igs` | The same Type 144 structure with outer flag 0 on an unbounded plane at z = 0, with the inner square from (0.25,0.25) to (0.75,0.75). |
