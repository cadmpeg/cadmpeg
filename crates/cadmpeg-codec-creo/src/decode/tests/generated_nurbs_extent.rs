// SPDX-License-Identifier: Apache-2.0
//! Tests: generated NURBS sweep extent carrier reconciliation.

use crate::decode::sweep::generated_nurbs_translation_extent;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn translated_surface() -> NurbsSurface {
    NurbsSurface::new(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 2.0)],
            vec![Point3::new(1.0, 1.0, 0.0), Point3::new(1.0, 1.0, 2.0)],
            vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 2.0)],
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
                    length: cadmpeg_ir::scalar::NonZeroLength::new(2.0)
                        .expect("nonzero length fixture"),
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
        kind,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, origin, normal| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                origin,
                normal,
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    };
    let local_plane =
        |surface_id, origin: [f64; 3], normal: [f64; 3]| crate::surface::PlaneLocalSystem {
            surface_id,
            body: Vec::new(),
            slots: [
                1.0, 0.0, 0.0, 0.0, 0.0, 0.0, normal[0], normal[1], normal[2], origin[0],
                origin[1], origin[2],
            ]
            .map(Some),
            layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
            classification: crate::surface::LocalSystemClassification::Simple,
            row_offset: 0,
            offset: 0,
        };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(
            31,
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        ),
        row(32, crate::surface::SurfaceKind::Plane),
        row(33, crate::surface::SurfaceKind::Plane),
        row(
            34,
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        ),
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

    scan.planes.local_systems[1].slots[9..12].copy_from_slice(&[Some(0.0), Some(0.0), Some(3.0)]);
    assert!(generated_nurbs_translation_extent(&scan, &ir, 7, None).is_none());
}
