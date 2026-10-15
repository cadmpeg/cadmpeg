// SPDX-License-Identifier: Apache-2.0

use super::super::saved_section_missing_line_geometry;
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

#[test]
fn missing_line_keeps_service_geometry_and_offset() {
    let (offset, geometry) =
        saved_section_missing_line_geometry(&fixture()).expect("missing line resolved");
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
