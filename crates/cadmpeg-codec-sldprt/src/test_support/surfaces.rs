// SPDX-License-Identifier: Apache-2.0
//! Surface fixtures.

use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

/// A B-surface 100 mm long, `u` knots 0.02 apart, one pole row raised 0.01 mm.
pub(crate) fn ridged() -> SurfaceGeometry {
    let mut u_knots = vec![0.0; 4];
    u_knots.extend((1..50).map(|i| 0.02 * f64::from(i)));
    u_knots.extend([1.0; 4]);
    let count = u_knots.len() - 4;
    let mut rows = Vec::new();
    for i in 0..count {
        let greville = (u_knots[i + 1] + u_knots[i + 2] + u_knots[i + 3]) / 3.0;
        let z = if i == 17 { 0.01 } else { 0.0 };
        rows.push(vec![
            Point3::new(100.0 * greville, 0.0, z),
            Point3::new(100.0 * greville, 30.0, z),
        ]);
    }
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(3, u_knots, false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(rows, None),
            false,
        )
        .expect("fixture constructor admission"),
    ))
}
