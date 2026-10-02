// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::test_support::with_points;
use cadmpeg_ir::sketches::SketchGeometry;
use std::collections::BTreeMap;

pub(super) fn resolved_section_reference_line_geometry(
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureReferenceLineSegment,
) -> Option<SketchGeometry> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::sketch::geometry::resolved_section_reference_line_geometry(
            ctx,
            definition,
            variable_points,
            points,
            segment,
        )
    })
    .expect("test reference line geometry")
}

pub(super) fn section_line_fixed_coordinate(
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<crate::decode::sketch::axis::SectionAxis> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::sketch::skamp::section_line_fixed_coordinate(ctx, definition, segment)
    })
    .expect("test fixed-coordinate graph")
}

pub(super) fn section_skamp_point_on_line(
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Option<(u32, u32, crate::decode::sketch::axis::SectionAxis)> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::sketch::skamp::section_skamp_point_on_line(ctx, definition, skamp)
    })
    .expect("test point-on-line")
}

pub(super) fn section_skamp_saved_point_on_line(
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Option<(u32, crate::decode::sketch::axis::SectionAxis, f64)> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::sketch::skamp::section_skamp_saved_point_on_line(ctx, definition, skamp)
    })
    .expect("test saved point-on-line")
}

pub(super) fn base_definition() -> crate::feature::definitions::FeatureDefinition {
    let segment = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        body: Vec::new(),
        offset: 40,
    };
    let arc = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Arc([2, 3]),
        directions: [None; 3],
        center_id: Some(4),
        arc_orientation: Some(1),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 13,
        body: Vec::new(),
        offset: 41,
    };
    let point = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Point(4),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 14,
        body: Vec::new(),
        offset: 42,
    };
    let other_line = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Line([5, 6]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 15,
        body: Vec::new(),
        offset: 43,
    };
    let other_arc = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Arc([5, 6]),
        directions: [None; 3],
        center_id: Some(7),
        arc_orientation: Some(1),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 16,
        body: Vec::new(),
        offset: 44,
    };
    let relations = crate::feature::definitions::FeatureRelationTable {
        declared_count: 3,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureRelation {
            relation_id: 8,
            used: 1,
            operands: vec![12, 4],
            operand_vectors: None,
            sign: 1,
            dimension_id: 0,
            relation_type: 99,
            body: Vec::new(),
            offset: 80,
        }],
        skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
            header: crate::feature::definitions::FeatureSolverTableHeader {
                declared_count: 15,
                entity_ref: 1,
                offset: 46,
            },
            rows: vec![
                crate::feature::definitions::FeatureSkamp {
                    id: 3,
                    kind: 1,
                    flags: 0,
                    status: 1,
                    items: vec![crate::feature::definitions::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    }],
                    offset: 50,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 4,
                    kind: 2,
                    flags: 0,
                    status: 1,
                    items: vec![crate::feature::definitions::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    }],
                    offset: 60,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 5,
                    kind: 7,
                    flags: 0,
                    status: 1,
                    items: vec![crate::feature::definitions::FeatureSkampItem {
                        entity_id: 12,
                        sense: 4,
                    }],
                    offset: 70,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 6,
                    kind: 1,
                    flags: 0,
                    status: 1,
                    items: vec![crate::feature::definitions::FeatureSkampItem {
                        entity_id: 13,
                        sense: 0,
                    }],
                    offset: 71,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 7,
                    kind: 0,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 2,
                        },
                    ],
                    offset: 72,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 8,
                    kind: 4,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 3,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 2,
                        },
                    ],
                    offset: 73,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 9,
                    kind: 14,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 2,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 3,
                        },
                    ],
                    offset: 74,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 10,
                    kind: 14,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                    ],
                    offset: 75,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 11,
                    kind: 3,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 14,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                    ],
                    offset: 76,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 12,
                    kind: 9,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 14,
                            sense: 0,
                        },
                    ],
                    offset: 77,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 13,
                    kind: 5,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 78,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 14,
                    kind: 7,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 79,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 15,
                    kind: 8,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 80,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 16,
                    kind: 6,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 13,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 16,
                            sense: 0,
                        },
                    ],
                    offset: 81,
                },
                crate::feature::definitions::FeatureSkamp {
                    id: 17,
                    kind: 17,
                    flags: 2,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 12,
                            sense: 2,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 15,
                            sense: 2,
                        },
                    ],
                    offset: 82,
                },
            ],
        }),
        triples: None,
        offset: 45,
    };
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(917),
            owner_feature_id: Some(40),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(with_points(
            crate::feature::definitions::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                offset: 89,
            },
            vec![
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(0.0),
                    v: Some(2.0),
                },
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 5,
                    u: Some(3.0),
                    v: Some(2.0),
                },
            ],
        )),
        segments: Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 5,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![segment, arc, point, other_line, other_arc])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 30,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::definitions::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureDimension {
                dimension_type: 2,
                value: crate::feature::definitions::DimensionValue::Resolved(3.0),
                value_body: Vec::new(),
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 42,
                references: None,
                offset: 75,
            }],
            offset: 74,
        }),
        relations: Some(relations),
        saved_section: None,
        offset: 0,
    }
}

pub(super) fn distance_definition(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> crate::feature::definitions::FeatureDefinition {
    let mut distance_definition = definition.clone();
    distance_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            rows[0].vertical_horizontal = Some(0);
        });
    let distance_relation = &mut distance_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    distance_relation.relation_type = 0;
    distance_relation.sign = 1;
    distance_relation.dimension_id = 0;
    distance_relation.operand_vectors = Some([
        [Some(1), Some(2), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    distance_definition
}
