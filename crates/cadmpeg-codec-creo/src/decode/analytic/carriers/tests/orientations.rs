// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::document::CadIr;

use super::super::native_face_orientations;

fn one_surface_row() -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id: 17,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 1,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

fn one_round_feature() -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 23,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    }
}

#[test]
fn native_face_orientation_indexes_preserve_service_values() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(one_surface_row());
    scan.features.rows.push(one_round_feature());
    let orientations = native_face_orientations(&scan, &CadIr::empty());
    assert_eq!(orientations.get(&17), Some(&true));
}
