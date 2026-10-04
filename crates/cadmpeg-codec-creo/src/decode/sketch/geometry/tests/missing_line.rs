// SPDX-License-Identifier: Apache-2.0

use super::super::saved_section_missing_line_geometry;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

fn fixture() -> crate::feature::definitions::FeatureDefinition {
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureOrderRow, FeatureOrderTable,
        FeatureSavedEntity, FeatureSavedLine, FeatureSavedSection, FeatureSegment,
        FeatureSegmentKind, FeatureSegmentTable, FeatureTrimEntity, FeatureTrimEntityTable,
        TrimEntityKind,
    };
    let segment = |external_id, offset, vertical_horizontal| FeatureSegment {
        kind: FeatureSegmentKind::Line([7, 9]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset,
    };
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![segment(42, 40, None), segment(43, 41, Some(1))]
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 4,
        }),
        trim_entities: Some(FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![FeatureTrimEntity {
                external_id: 43,
                mode: Some(0),
                vertices: [3, 4],
                kind: TrimEntityKind::Line,
                offset: 7,
            }],
            solved_external_ids: vec![43],
            offset: 5,
        }),
        trim_vertices: None,
        order_table: Some(FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 10,
            }],
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(FeatureSavedSection {
            entities: vec![FeatureSavedEntity::Line(FeatureSavedLine {
                entity_id: 3,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(-8.0), Some(-0.85), Some(0.0)],
                    [Some(8.0), Some(-0.85), None],
                ],
                body: Vec::new(),
                offset: 20,
            })],
            offset: 18,
        }),
        offset: 0,
    }
}

fn run(policy: &DecodePolicy) -> Result<Option<(usize, SketchGeometry)>, CodecError> {
    let definition = fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    saved_section_missing_line_geometry(&ctx, &definition)
}

fn assert_item_refusal(limit: u64, operation: &'static str) {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let error = run(&policy).expect_err("missing-line collection exceeds cap");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn missing_line_refuses_trimmed_id_tree_node() {
    assert_item_refusal(0, "creo missing-line trimmed ID nodes");
}

#[test]
fn missing_line_refuses_saved_geometry_vec_growth() {
    assert_item_refusal(1, "creo missing-line saved geometries");
}

#[test]
fn missing_line_refuses_ordered_id_tree_node() {
    assert_item_refusal(2, "creo missing-line ordered ID nodes");
}

#[test]
fn missing_line_refuses_geometry_id_tree_node() {
    assert_item_refusal(3, "creo missing-line geometry ID nodes");
}

#[test]
fn missing_line_refuses_endpoint_vec_growth() {
    assert_item_refusal(5, "creo missing-line endpoints");
}

#[test]
fn missing_line_refuses_endpoint_pair_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let error = run(&policy).expect_err("two endpoints need two ordered comparisons");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo missing-line endpoint pairs"));
}

#[test]
fn missing_line_keeps_service_geometry_and_offset() {
    let (offset, geometry) = run(&DecodePolicy::service())
        .expect("service resources")
        .expect("missing line resolved");
    assert_eq!(offset, 41);
    assert_eq!(
        geometry,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
        .expect("line geometry")
    );
}
