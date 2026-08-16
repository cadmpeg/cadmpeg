// SPDX-License-Identifier: Apache-2.0

use super::schema_feature_definition;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureDefinition as IrFeatureDefinition;
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;

#[test]
fn datum_feature_rejects_conflicting_local_and_transferred_plane_carriers() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: 1,
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
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#6".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 1.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, 923, "Datum Plane"),
        IrFeatureDefinition::DatumPlane { .. }
    ));

    match &mut ir.model.surfaces[0].geometry {
        SurfaceGeometry::Plane { origin, .. } => origin.y = 2.0,
        _ => panic!("transferred datum plane"),
    }
    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, 923, "Datum Plane"),
        IrFeatureDefinition::DatumPlaneUnresolved
    );
}
