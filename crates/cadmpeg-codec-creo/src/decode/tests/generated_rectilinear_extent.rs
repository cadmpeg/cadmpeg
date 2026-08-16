// SPDX-License-Identifier: Apache-2.0
//! Tests: rectilinear sweep extent carrier reconciliation.

use crate::decode::sweep::generated_rectilinear_plane_extent;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, Termination};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;

fn expected_extent() -> (ExtrudeExtent, [f64; 3]) {
    (
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(42.0),
                },
                draft: None,
                offset: None,
            },
        },
        [0.0, -1.0, 0.0],
    )
}

fn section() -> crate::feature::FeatureSection3d {
    crate::feature::FeatureSection3d {
        sketch_plane_entity_id: Some(30),
        sketch_plane_flip: Some(crate::feature::BinaryFlag::Clear),
        reference_plane_entity_ids: vec![29],
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: crate::feature::FeatureSectionOrientation {
            section_flip: Some(crate::feature::BinaryFlag::Set),
            ..Default::default()
        },
        dimension_ids: Vec::new(),
        offset: 0,
    }
}

#[test]
fn rectilinear_extent_reconciles_native_and_transferred_planes() {
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, origin, normal| Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(37, false),
        row(31, false),
        row(32, true),
        row(33, true),
        row(34, true),
        row(36, false),
        row(35, true),
    ]);
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("creo:visibgeom:surface#37".to_string()),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        plane(31, Point3::new(0.0, 6.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(32, Point3::new(0.0, 48.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(33, Point3::new(4.0, 48.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        plane(
            34,
            Point3::new(-4.0, 48.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(36, Point3::new(0.0, 30.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(35, Point3::new(0.0, 48.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
    ]);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section())),
        Some(expected_extent())
    );

    scan.planes
        .local_systems
        .push(crate::surface::PlaneLocalSystem {
            surface_id: 32,
            body: Vec::new(),
            slots: Vec::new(),
            origin: Some([0.0, 48.0, 0.0]),
            u_axis: Some([0.0, 0.0, 1.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: crate::surface::LocalSystemClassification::Simple,
            row_offset: 0,
            offset: 0,
        });
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section())),
        Some(expected_extent())
    );

    let mut local_only = ir.clone();
    local_only
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == SurfaceId("creo:visibgeom:surface#32".to_string()))
        .expect("plane surface")
        .geometry = SurfaceGeometry::Unknown { record: None };
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &local_only, 7, Some(&section())),
        Some(expected_extent())
    );

    scan.planes.local_systems[0].origin = Some([0.0, 49.0, 0.0]);
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section())).is_none());
}
