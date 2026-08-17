// SPDX-License-Identifier: Apache-2.0

use super::super::equations::PlaneEquation;
use super::{existing_plane_agrees_with_topology, placed_carriers, transfer_topology_bound_planes};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;

fn carrier_surface(id: u32, geometry: SurfaceGeometry) -> Surface {
    Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry,
        source_object: None,
    }
}

fn cylinder_surface(id: u32, radius: f64) -> Surface {
    carrier_surface(
        id,
        SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        },
    )
}

fn carrier_row(id: u32, kind: crate::surface::SurfaceKind) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 1,
        reversed: false,
        boundary_type: 1,
        next_surface: 0,
        offset: 0,
    }
}

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

#[test]
fn placed_carriers_reject_duplicate_model_surface_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(7, crate::surface::SurfaceKind::Cylinder));
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .surfaces
        .extend([cylinder_surface(7, 2.0), cylinder_surface(7, 3.0)]);

    assert!(!placed_carriers(&scan, &ir).contains_key(&7));
}

#[test]
fn duplicate_model_surface_ids_remove_native_carrier() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(7, crate::surface::SurfaceKind::Plane));
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 7,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 0,
        });
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        carrier_surface(
            7,
            SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
        carrier_surface(
            7,
            SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 1.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
    ]);

    assert!(!placed_carriers(&scan, &ir).contains_key(&7));
}

#[test]
fn topology_bound_plane_rejects_duplicate_model_curve_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(5, crate::surface::SurfaceKind::Plane));
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 1,
            directions: [0; 2],
            faces: [5, 0],
            next_edges: [11, 0],
            offset: 20,
        });
    scan.topology.loops.push(crate::topology::Loop {
        face_id: 5,
        half_edges: vec![crate::topology::HalfEdgeId {
            curve_id: 11,
            side: 0,
        }],
    });

    let curve = Curve {
        id: CurveId("creo:visibgeom:curve#11".to_string()),
        geometry: CurveGeometry::Circle {
            center: Point3::new(2.0, 3.0, 4.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.extend([curve.clone(), curve]);

    assert_eq!(
        transfer_topology_bound_planes(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
        ),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}
