// SPDX-License-Identifier: Apache-2.0

use super::super::equations::PlaneEquation;
use super::existing_plane_agrees_with_topology;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

fn topology_plane() -> PlaneEquation {
    PlaneEquation {
        origin: [0.0, 0.0, 4.0],
        normal: [0.0, 0.0, 1.0],
    }
}

#[test]
fn existing_plane_carrier_accepts_reversed_normal() {
    let existing = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 4.0),
        normal: Vector3::new(0.0, 0.0, -1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        Some(true)
    );
}

#[test]
fn existing_plane_carrier_rejects_offset_conflict() {
    let existing = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 5.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        Some(false)
    );
}

#[test]
fn existing_unknown_carrier_does_not_compete_with_topology() {
    let existing = SurfaceGeometry::Unknown { record: None };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        None
    );
}

#[test]
fn existing_non_plane_carrier_conflicts_with_topology() {
    let existing = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 4.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        Some(false)
    );
}
