// SPDX-License-Identifier: Apache-2.0
//! The depth the semantic writer walks over an inline surface basis.

use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry, MAX_GEOMETRY_NESTING};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use crate::writer::surface_reference;

/// A plane whose `u_axis` is not the direction the writer states for a carrier
/// it declines to walk, so the two outcomes are distinguishable.
fn placed_plane(placements: usize) -> Result<SolvedSurfaceGeometry, &'static str> {
    let mut geometry = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .expect("a unit-axis plane"),
    );
    for _ in 0..placements {
        geometry = SolvedSurfaceGeometry::Transformed(PlacedSurface::try_new(
            Box::new(geometry),
            Transform::identity(),
        )?);
    }
    Ok(geometry)
}

#[test]
fn a_surface_placement_chain_past_the_bound_cannot_reach_the_writer() {
    let accepted = placed_plane(MAX_GEOMETRY_NESTING).expect("admitted nesting");
    assert_eq!(
        surface_reference(&accepted),
        Vector3::new(0.0, 1.0, 0.0),
        "a chain at the admitted depth states the basis reference"
    );

    assert_eq!(
        placed_plane(MAX_GEOMETRY_NESTING + 1),
        Err("PlacedSurface.basis nests past the admitted inline basis depth"),
        "a chain past the admitted depth cannot be built at all"
    );
}
