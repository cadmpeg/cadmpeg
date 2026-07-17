<!-- Generated from crates/cadmpeg-step/src/entity_table.rs. Do not edit by hand; run `cargo test -p cadmpeg-step`. -->

# STEP writer carrier dispatch

The writer maps each IR geometry carrier to its most-specific STEP entity subtype. Rational NURBS carriers realize as AND-combined complex instances; the supertype list records the emission order.

## Surfaces

| IR variant | STEP entity | Form | Mandatory fields |
|---|---|---|---|
| Plane | `PLANE` | simple | position |
| Cylinder | `CYLINDRICAL_SURFACE` | simple | position, radius |
| Cone | `CONICAL_SURFACE` | simple | position, radius, semi_angle |
| Sphere | `SPHERICAL_SURFACE` | simple | position, radius |
| Torus | `TOROIDAL_SURFACE` | simple | position, major_radius, minor_radius |
| Nurbs (non-rational) | `B_SPLINE_SURFACE_WITH_KNOTS` | simple | u_degree, v_degree, control_points_list, surface_form, u_closed, v_closed, self_intersect, u_multiplicities, v_multiplicities, u_knots, v_knots, knot_spec |
| Nurbs (rational) | `B_SPLINE_SURFACE_WITH_KNOTS` | complex: BOUNDED_SURFACE B_SPLINE_SURFACE B_SPLINE_SURFACE_WITH_KNOTS GEOMETRIC_REPRESENTATION_ITEM RATIONAL_B_SPLINE_SURFACE REPRESENTATION_ITEM SURFACE | weights |

## Curves

| IR variant | STEP entity | Form | Mandatory fields |
|---|---|---|---|
| Line | `LINE` | simple | pnt, dir |
| Circle | `CIRCLE` | simple | position, radius |
| Ellipse | `ELLIPSE` | simple | position, semi_axis_1, semi_axis_2 |
| Parabola | `PARABOLA` | simple | position, focal_dist |
| Hyperbola | `HYPERBOLA` | simple | position, semi_axis, imaginary_semi_axis |
| Degenerate | `POLYLINE` | simple | points |
| Nurbs (non-rational) | `B_SPLINE_CURVE_WITH_KNOTS` | simple | degree, control_points_list, curve_form, closed_curve, self_intersect, knot_multiplicities, knots, knot_spec |
| Nurbs (rational) | `B_SPLINE_CURVE_WITH_KNOTS` | complex: BOUNDED_CURVE B_SPLINE_CURVE B_SPLINE_CURVE_WITH_KNOTS CURVE GEOMETRIC_REPRESENTATION_ITEM RATIONAL_B_SPLINE_CURVE REPRESENTATION_ITEM | weights |

## Supporting entities

| IR variant | STEP entity | Form | Mandatory fields |
|---|---|---|---|
| Point3 | `CARTESIAN_POINT` | simple | coordinates |
| Vector3 (unit) | `DIRECTION` | simple | direction_ratios |
| Vector3 (with magnitude) | `VECTOR` | simple | orientation, magnitude |
| Placement | `AXIS2_PLACEMENT_3D` | simple | location, axis, ref_direction |

