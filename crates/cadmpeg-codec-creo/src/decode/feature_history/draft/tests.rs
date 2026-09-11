// SPDX-License-Identifier: Apache-2.0

use super::{schema_feature_definition, unbounded_feature_plane_definition};
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::UnresolvedFamily;
use cadmpeg_ir::features::{
    FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn datum_feature_rejects_conflicting_local_and_transferred_plane_carriers() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 6,
            origin: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [0.0, 0.0, 1.0],
            offset: 1,
        });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, Some(SchemaClass::DatumPlane), "Datum Plane")
            .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane { .. })
    ));

    match &mut ir.model.surfaces[0].geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
            let mut origin = *origin;
            origin.y = 2.0;
            *plane_surface = cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis)
                .expect("valid PlaneSurface fixture");
        }
        _ => panic!("transferred datum plane"),
    }
    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, Some(SchemaClass::DatumPlane), "Datum Plane")
            .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane
        })
    );
}

fn unbounded_plane_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    scan
}

fn plane_surface(origin_y: f64) -> Surface {
    Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, origin_y, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    }
}

fn placed_plane() -> crate::surface::OutlinePlane {
    crate::surface::OutlinePlane {
        surface_id: 6,
        origin: [0.0, 1.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [0.0, 0.0, 1.0],
        offset: 1,
    }
}

#[test]
fn unbounded_plane_uses_its_placed_carrier_without_model_surface() {
    let mut scan = unbounded_plane_scan();
    scan.planes.positional_frames.push(placed_plane());

    assert_eq!(
        unbounded_feature_plane_definition(&scan, &CadIr::empty(), 5),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0)
                )
                .expect("valid test fixture"),
            }
        ))
    );
}

#[test]
fn unbounded_plane_uses_its_model_carrier_without_placed_surface() {
    let scan = unbounded_plane_scan();
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(plane_surface(1.0));

    assert_eq!(
        unbounded_feature_plane_definition(&scan, &ir, 5),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0)
                )
                .expect("valid test fixture"),
            }
        ))
    );
}

#[test]
fn unbounded_plane_rejects_conflicting_carriers() {
    let mut scan = unbounded_plane_scan();
    scan.planes.positional_frames.push(placed_plane());
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(plane_surface(2.0));

    assert!(unbounded_feature_plane_definition(&scan, &ir, 5).is_none());
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, None, "Unbounded Plane")
            .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Native { .. })
    ));
}
