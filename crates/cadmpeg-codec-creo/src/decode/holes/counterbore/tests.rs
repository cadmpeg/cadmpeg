// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

fn boundary_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 0,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 42,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 2,
        },
    ]);
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 42,
            directions: [1, 1],
            faces: [2, 1],
            next_edges: [11, 11],
            offset: 11,
        });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 1,
        });
    scan
}

fn boundary_circle() -> cadmpeg_ir::geometry::Curve {
    cadmpeg_ir::geometry::Curve {
        id: CurveId("creo:visibgeom:curve#11".to_string()),
        geometry: CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        },
        source_object: None,
    }
}

fn model_plane(origin: [f64; 3]) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: SurfaceId("creo:visibgeom:surface#1".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: origin.into(),
            normal: [0.0, 0.0, 1.0].into(),
            u_axis: [1.0, 0.0, 0.0].into(),
        },
        source_object: None,
    }
}

#[test]
fn boundary_circle_uses_native_plane_carrier_when_model_plane_is_absent() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.curves.push(boundary_circle());

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        Some((1, Point3::new(0.0, 0.0, 0.0), [0.0, 0.0, 1.0]))
    );
}

#[test]
fn boundary_circle_uses_model_plane_carrier_when_native_plane_is_absent() {
    let mut scan = boundary_scan();
    scan.planes.positional_frames.clear();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.curves.push(boundary_circle());
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        Some((1, Point3::new(0.0, 0.0, 0.0), [0.0, 0.0, 1.0]))
    );
}

#[test]
fn boundary_circle_rejects_conflicting_model_plane_carrier() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.curves.push(boundary_circle());
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.5]));

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        None
    );
}
