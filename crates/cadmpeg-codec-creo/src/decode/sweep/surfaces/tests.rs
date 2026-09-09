// SPDX-License-Identifier: Apache-2.0

use super::*;

fn surface_row(
    id: u32,
    feature_id: u32,
    kind: crate::surface::SurfaceKind,
) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        kind,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

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

fn saved_spline_definition() -> crate::feature::FeatureDefinition {
    crate::feature::FeatureDefinition {
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
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Spline(
                crate::feature::FeatureSavedSpline {
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
    let mut scan = crate::container::scan_bytes(Vec::new());
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
