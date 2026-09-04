// SPDX-License-Identifier: Apache-2.0
//! Tests: generated NURBS sweep extent carrier reconciliation.

use crate::decode::sweep::generated_nurbs_translation_extent;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, LinearTermination};
use cadmpeg_ir::geometry::{NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn translated_surface() -> NurbsSurface {
    NurbsSurface::new(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 2.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 2.0),
        ],
        None,
        false,
        false,
        false,
    )
    .expect("valid translated surface")
}

fn expected_extent() -> (ExtrudeExtent, [f64; 3]) {
    (
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: Length(2.0),
                },
                draft: None,
            },
        },
        [0.0, 0.0, 1.0],
    )
}

#[test]
fn generated_nurbs_extent_reconciles_native_and_transferred_planes() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 7,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, origin, normal| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let local_plane = |surface_id, origin, normal| crate::surface::PlaneLocalSystem {
        surface_id,
        body: Vec::new(),
        slots: Vec::new(),
        origin: Some(origin),
        u_axis: Some([1.0, 0.0, 0.0]),
        normal: Some(normal),
        classification: crate::surface::LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(31, crate::surface::SurfaceKind::Extrusion),
        row(32, crate::surface::SurfaceKind::Plane),
        row(33, crate::surface::SurfaceKind::Plane),
        row(34, crate::surface::SurfaceKind::Extrusion),
        row(35, crate::surface::SurfaceKind::Plane),
    ]);
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#31".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Nurbs(translated_surface()),
            source_object: None,
        },
        plane(32, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        plane(33, Point3::new(0.0, 0.0, 2.0), Vector3::new(0.0, 0.0, -1.0)),
        Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#34".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#35".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
    ]);
    assert_eq!(
        generated_nurbs_translation_extent(&scan, &ir, 7, None),
        Some(expected_extent())
    );

    scan.planes.local_systems.extend([
        local_plane(32, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        local_plane(33, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
    ]);
    assert_eq!(
        generated_nurbs_translation_extent(&scan, &ir, 7, None),
        Some(expected_extent())
    );

    let mut local_only = ir.clone();
    for surface_id in [32, 33] {
        local_only
            .model
            .surfaces
            .iter_mut()
            .find(|surface| {
                surface.id
                    == SurfaceId::mint(format!("creo:visibgeom:surface#{surface_id}"))
                        .expect("identity grammar")
            })
            .expect("plane surface")
            .geometry = SurfaceGeometry::Unknown { record: None };
    }
    assert_eq!(
        generated_nurbs_translation_extent(&scan, &local_only, 7, None),
        Some(expected_extent())
    );

    scan.planes.local_systems[1].origin = Some([0.0, 0.0, 3.0]);
    assert!(generated_nurbs_translation_extent(&scan, &ir, 7, None).is_none());
}
