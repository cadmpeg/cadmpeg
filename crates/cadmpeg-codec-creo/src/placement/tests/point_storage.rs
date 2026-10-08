// SPDX-License-Identifier: Apache-2.0
//! Placement reconciliation maps are scratch, including rejected candidates.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn point_definition() -> FeatureDefinition {
    FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(917),
            owner_feature_id: Some(10),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::definitions::test_support::with_points(
            FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                offset: 0,
            },
            vec![
                FeatureSectionPoint { point_id: 1, u: Some(0.0), v: Some(0.0) },
                FeatureSectionPoint { point_id: 2, u: Some(1.0), v: Some(0.0) },
            ],
        )),
        segments: Some(FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: [crate::feature::segment_rows::SegmentRow::Ordinary(FeatureSegment {
                kind: FeatureSegmentKind::Line([1, 2]),
                directions: [None; 3],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 5,
                body: Vec::new(),
                offset: 0,
            })].into_iter().collect(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

#[test]
fn placement_point_reconciliation_does_not_retain_scratch_maps() {
    let definition = point_definition();
    let definitions = [definition];
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(vec![SurfaceRow {
        id: 5,
        kind: SurfaceKind::Plane,
        feature_id: 10,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }]);
    let sources = PlacementSources {
        datums: &[], surface_rows: &rows, model_planes: &[], outline_planes: &[],
        plane_envelopes: &[], surface_parameters: &[], geometry_tables: &[], affected_ids: &[],
    };
    let transforms = [FeatureSectionTransform::new(
        917, Some(10), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0,
    ).expect("section frame")];
    for case in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut lookup = super::super::PlacementLookup::new(&ctx, &sources, &definitions)
            .expect("empty lookup");
        match case {
            0 => assert!(parse_generated_cylinder_section_transform(
                &ctx, &definitions[0], &[], &mut lookup,
            ).expect("cylinder point scratch").is_none()),
            1 => assert!(parse_generated_planar_section_transform(
                &ctx, &definitions[0], &[], &mut lookup,
            ).expect("planar point scratch").is_none()),
            _ => assert!(super::super::feature_generated_plane_equation(
                &ctx, 5, &transforms, &mut lookup,
            ).expect("generated plane point scratch").is_some()),
        }
    }
}
