// SPDX-License-Identifier: Apache-2.0
//! The depth the semantic writer walks over an inline surface basis.

use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, MAX_GEOMETRY_NESTING};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use crate::writer::surface_reference;

/// A plane whose `u_axis` is not the direction the writer states for a carrier
/// it declines to walk, so the two outcomes are distinguishable.
fn placed_plane(placements: usize) -> SolvedSurfaceGeometry {
    let mut geometry = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .expect("a unit-axis plane"),
    );
    for _ in 0..placements {
        geometry = SolvedSurfaceGeometry::Transformed {
            basis: Box::new(geometry),
            transform: Transform::identity(),
        };
    }
    geometry
}

#[test]
fn a_surface_placement_chain_past_the_bound_is_not_walked() {
    let accepted = placed_plane(MAX_GEOMETRY_NESTING);
    assert_eq!(
        surface_reference(&accepted),
        Vector3::new(0.0, 1.0, 0.0),
        "a chain at the admitted depth still states the basis reference"
    );

    let refused = placed_plane(MAX_GEOMETRY_NESTING + 1);
    assert_eq!(
        surface_reference(&refused),
        Vector3::new(1.0, 0.0, 0.0),
        "a chain past the admitted depth is not walked"
    );
}
