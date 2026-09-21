// SPDX-License-Identifier: Apache-2.0
//! A transformed surface carrier leaves the semantic writer as an error, and
//! the writer reads no basis under it.

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{
    PlacedSurface, SolvedSurfaceGeometry, SurfaceGeometry, MAX_GEOMETRY_NESTING,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use crate::writer::{surface_reference, surface_values};

/// The direction `surface_reference` states for a carrier whose own frame it
/// does not read.
const DEFAULT_REFERENCE: Vector3 = Vector3 {
    x: 1.0,
    y: 0.0,
    z: 0.0,
};

/// A plane whose `u_axis` is not [`DEFAULT_REFERENCE`], so a walk of the basis
/// and a refusal to walk it are distinguishable.
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
fn a_plane_states_its_own_reference_direction() {
    let plane = placed_plane(0).expect("a leaf plane");
    assert_eq!(
        surface_reference(&plane),
        Vector3::new(0.0, 1.0, 0.0),
        "a plane states its u axis"
    );
}

#[test]
fn a_transformed_surface_is_unwritable_at_every_admitted_depth() {
    for placements in [1, 2, MAX_GEOMETRY_NESTING] {
        let geometry = placed_plane(placements).expect("admitted nesting");
        assert_eq!(
            surface_reference(&geometry),
            DEFAULT_REFERENCE,
            "a transformed carrier states the default direction and no basis is read"
        );
        let refusal = surface_values(
            &SurfaceGeometry::Solved(geometry),
            Vector3::new(0.0, 1.0, 0.0),
            1.0,
        )
        .expect_err("a transformed surface carrier is unwritable");
        assert!(
            matches!(refusal, CodecError::NotImplemented(_)),
            "the refusal is the writer's unsupported-carrier route, not {refusal:?}"
        );
    }
}
