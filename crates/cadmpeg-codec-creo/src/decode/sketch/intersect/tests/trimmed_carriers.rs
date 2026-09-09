// SPDX-License-Identifier: Apache-2.0

use super::super::trimmed_section_segment_geometry_with_missing_line;
use crate::decode::sketch::{resolved_section_points, saved_section_missing_line_geometry};
use crate::decode::tests::declared_solver_rows;
use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::BTreeMap;

fn trimmed_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let missing_line = saved_section_missing_line_geometry(definition);
    trimmed_section_segment_geometry_with_missing_line(
        definition,
        points,
        trim_vertices,
        segment,
        missing_line.as_ref(),
    )
}

#[test]
fn trimmed_line_reconciles_carrier_and_solver_orientation() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line([7, 9]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let anchor = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line([5, 6]),
        external_id: 41,
        body: Vec::new(),
        offset: 39,
        ..segment.clone()
    };
    let horizontal = crate::feature::FeatureSkamp {
        id: 1,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 50,
    };
    let parallel = crate::feature::FeatureSkamp {
        id: 2,
        kind: 7,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 41,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 42,
                sense: 0,
            },
        ],
        offset: 55,
    };
    let mut definition = crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![anchor, segment.clone()])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 20,
        }),
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                kind: crate::feature::TrimEntityKind::Line,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: Some(crate::feature::FeatureRelationTable {
            declared_count: 2,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                header: crate::feature::definitions::FeatureSolverTableHeader {
                    declared_count: 2,
                    entity_ref: 70,
                    offset: 45,
                },
                rows: vec![horizontal, parallel],
            }),
            triples: None,
            offset: 44,
        }),
        saved_section: None,
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 3.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &BTreeMap::new(), &trim_vertices, &segment,),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-2.0, 3.0),
                end: cadmpeg_ir::math::Point2::new(4.0, 3.0),
            })
            .expect("valid test fixture")
        )
    );
    let mut disabled_parallel = definition.clone();
    disabled_parallel
        .relations
        .as_mut()
        .expect("solver relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[1]
        .status = 34;
    assert_eq!(
        trimmed_section_segment_geometry(
            &disabled_parallel,
            &BTreeMap::new(),
            &trim_vertices,
            &segment,
        ),
        None
    );

    let carrier_points = BTreeMap::from([(7, [0.0, 3.0]), (9, [2.0, 3.0])]);
    assert!(trimmed_section_segment_geometry(
        &definition,
        &carrier_points,
        &trim_vertices,
        &segment,
    )
    .is_some());
    let off_carrier_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 4.0])]);
    assert!(trimmed_section_segment_geometry(
        &definition,
        &carrier_points,
        &off_carrier_vertices,
        &segment,
    )
    .is_none());

    let relations = definition.relations.as_mut().expect("solver relations");
    declared_solver_rows(&mut relations.skamps).push(crate::feature::FeatureSkamp {
        id: 3,
        kind: 2,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 60,
    });
    relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("solver header")
        .declared_count = 3;
    assert!(trimmed_section_segment_geometry(
        &definition,
        &BTreeMap::new(),
        &trim_vertices,
        &segment,
    )
    .is_none());
}

#[test]
fn arc_carriers_use_trim_vertices() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc([7, 9]),
        directions: [None; 3],
        center_id: Some(8),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                kind: crate::feature::TrimEntityKind::Arc { center_vertex: 3 },
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
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
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Arc(
                crate::feature::FeatureSavedArc {
                    entity_id: 3,
                    center: [Some(0.0), Some(0.0), Some(0.0)],
                    radius: Some(2.0),
                    endpoints: [[None; 3]; 2],
                    parameters: [None; 2],
                    body: Vec::new(),
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 0.0]), (2, [0.0, -2.0])]);
    let points = BTreeMap::from([(7, [2.0, 0.0]), (8, [0.0, 0.0]), (9, [0.0, 2.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &points, &trim_vertices, &segment),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(-std::f64::consts::FRAC_PI_2),
                end_angle: Angle(std::f64::consts::PI),
            })
            .expect("valid test fixture")
        )
    );

    let mut var_segment = segment.clone();
    var_segment.radius_ref = Some(10);
    let mut var_arc = definition;
    var_arc.variables = Some(crate::feature::definitions::test_support::with_points(
        crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 5,
        },
        vec![
            crate::feature::FeatureSectionPoint {
                point_id: 7,
                u: Some(2.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 8,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 9,
                u: Some(0.0),
                v: Some(2.0),
            },
        ],
    ));
    var_arc.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![var_segment.clone()])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 6,
    });
    var_arc.order_table = None;
    var_arc.saved_section = None;
    assert_eq!(
        trimmed_section_segment_geometry(
            &var_arc,
            &resolved_section_points(&var_arc),
            &trim_vertices,
            &var_segment,
        ),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(-std::f64::consts::FRAC_PI_2),
                end_angle: Angle(std::f64::consts::PI),
            })
            .expect("valid test fixture")
        )
    );
}
