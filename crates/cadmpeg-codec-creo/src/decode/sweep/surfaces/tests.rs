// SPDX-License-Identifier: Apache-2.0

use super::{transfer_saved_spline_curves, unique_feature_surface_row};
use crate::decode::tests::surface_row;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn generated_surface_binding_requires_one_matching_row() {
    let row = surface_row(31, 7, crate::surface::SurfaceKind::Plane);
    assert!(unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        8,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Cylinder,
    ));
    assert!(!unique_feature_surface_row(
        &[row.clone(), row],
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
}

fn saved_spline_definition() -> crate::feature::definitions::FeatureDefinition {
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: Some(40),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(crate::feature::definitions::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        }),
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Spline(
                crate::feature::definitions::FeatureSavedSpline {
                    entity_id: Some(1),
                    declared_point_count: Some(2),
                    interpolation_points: vec![[2.0, 0.0, 0.0], [2.0, 0.0, 1.0]],
                    interpolation_points_body: Vec::new(),
                    endpoint_tangents: Some(crate::feature::definitions::DecodedField {
                        value: [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
                        body: Vec::new(),
                    }),
                    parameters: Some(crate::feature::definitions::DecodedField {
                        value: Vec::new(),
                        body: Vec::new(),
                    }),
                    offset: 0,
                },
            )],
            offset: 0,
        }),
        offset: 0,
    }
}

#[test]
fn malformed_saved_spline_reports_transfer_loss() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.definitions.push(saved_spline_definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section frame"),
    );
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    assert_eq!(
        transfer_saved_spline_curves(&scan, &mut ir, &mut AnnotationBuilder::new(), &mut losses)
            .expect("transfer"),
        0
    );
    assert!(ir.model.curves.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::SectionSplineUnresolved.kind()
    );
}
