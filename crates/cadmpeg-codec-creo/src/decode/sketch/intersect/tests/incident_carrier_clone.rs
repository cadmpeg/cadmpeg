// SPDX-License-Identifier: Apache-2.0

use super::super::resolved_trim_vertex_coordinates;
use crate::feature::definitions::{
    DefinitionIdentity, FeatureDefinition, FeatureSegment, FeatureSegmentKind, FeatureSegmentTable,
    FeatureTrimEntity, FeatureTrimEntityTable, TrimEntityKind,
};
use std::collections::BTreeMap;

fn incident_line_fixture() -> (FeatureDefinition, BTreeMap<u32, [f64; 2]>) {
    let segment = |external_id, point_ids| FeatureSegment {
        kind: FeatureSegmentKind::Line(point_ids),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(1),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![segment(42, [1, 2]), segment(43, [2, 3])]
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 0,
        }),
        trim_entities: Some(FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![
                FeatureTrimEntity {
                    external_id: 42,
                    mode: None,
                    vertices: [10, 11],
                    kind: TrimEntityKind::Line,
                    offset: 0,
                },
                FeatureTrimEntity {
                    external_id: 43,
                    mode: None,
                    vertices: [10, 12],
                    kind: TrimEntityKind::Line,
                    offset: 0,
                },
            ],
            solved_external_ids: vec![42, 43],
            offset: 0,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let points = BTreeMap::from([(1, [0.0, 0.0]), (2, [1.0, 0.0]), (3, [1.0, 1.0])]);
    (definition, points)
}

#[test]
fn incident_carrier_references_admit_work_and_preserve_service_result() {
    let (definition, points) = incident_line_fixture();
    // The shared endpoint fixes vertex 10; each incident line fixes its other endpoint.
    let expected = BTreeMap::from([(10, [1.0, 0.0]), (11, [0.0, 0.0]), (12, [1.0, 1.0])]);
    let coordinates = crate::test_support::assert_work_boundaries(
        &["creo incident carrier IDs", "creo sketch intersection carrier lookup"],
        |ctx| resolved_trim_vertex_coordinates(ctx, &definition, &points, &BTreeMap::new()),
    );
    assert_eq!(coordinates, expected);
}
