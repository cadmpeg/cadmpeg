// SPDX-License-Identifier: Apache-2.0

use super::super::{declared_solver_rows, synchronize_segment_count, synchronize_skamp_count};
use super::fixtures::{
    base_definition, section_line_fixed_coordinate, section_skamp_point_on_line,
};
use crate::decode::records::sketch_section_point_records;
use crate::decode::sketch::coordinates::{resolved_section_coordinates, resolved_section_points};
use crate::decode::sketch::skamp::unique_section_skamp_segment;
use crate::feature::definitions::test_support::{append_points, with_points};
use crate::feature::definitions::{ScalarLane, VariableType};
use std::collections::BTreeMap;

#[test]
fn section_solver_point_coordinates_merge_compatible_and_reject_conflicting_incidences() {
    let definition = base_definition();
    let mut coincident_definition = definition.clone();
    coincident_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            crate::feature::definitions::FeatureSegment {
                kind: crate::feature::definitions::FeatureSegmentKind::Line([7, 8]),
                directions: [None; 3],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 17,
                body: Vec::new(),
                offset: 87,
            },
        ));
    synchronize_segment_count(&mut coincident_definition);
    coincident_definition.variables = Some(with_points(
        crate::feature::definitions::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        },
        vec![
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 2,
                u: Some(3.0),
                v: Some(4.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 5,
                u: None,
                v: None,
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 4,
                u: None,
                v: Some(9.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 6,
                u: None,
                v: None,
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 7,
                u: Some(2.0),
                v: Some(6.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 8,
                u: None,
                v: None,
            },
        ],
    ));
    *declared_solver_rows(
        &mut coincident_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![
        crate::feature::definitions::FeatureSkamp {
            id: 17,
            kind: 0,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 3,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 15,
                    sense: 2,
                },
            ],
            offset: 83,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 18,
            kind: 3,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 15,
                    sense: 3,
                },
            ],
            offset: 84,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 19,
            kind: 2,
            flags: 0,
            status: 1,
            items: vec![crate::feature::definitions::FeatureSkampItem {
                entity_id: 15,
                sense: 0,
            }],
            offset: 85,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 20,
            kind: 9,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 15,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
            ],
            offset: 86,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 21,
            kind: 1,
            flags: 0,
            status: 1,
            items: vec![crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            }],
            offset: 88,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 22,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 17,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 17,
                    sense: 3,
                },
            ],
            offset: 89,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 23,
            kind: 5,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 17,
                    sense: 0,
                },
            ],
            offset: 90,
        },
    ];
    synchronize_skamp_count(&mut coincident_definition);
    let related_line = unique_section_skamp_segment(&coincident_definition, 17).expect("line");
    assert_eq!(
        section_line_fixed_coordinate(&coincident_definition, related_line),
        Some(crate::decode::sketch::axis::SectionAxis::U)
    );
    let coincident_points = crate::decode::with_test_decode_ctx(|ctx| {
        resolved_section_points(ctx, &coincident_definition)
    })
    .expect("test section solve");
    assert_eq!(coincident_points.get(&5), Some(&[3.0, 4.0]));
    assert_eq!(coincident_points.get(&4), Some(&[3.0, 9.0]));
    assert_eq!(coincident_points.get(&6), Some(&[3.0, 9.0]));
    assert_eq!(coincident_points.get(&8), Some(&[2.0, 2.0]));
    let mut disabled_incidences = coincident_definition.clone();
    for skamp in disabled_incidences
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()
    {
        skamp.status = 34;
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &disabled_incidences
        ))
        .expect("test section solve"),
        BTreeMap::from([(2, [3.0, 4.0]), (7, [2.0, 6.0])])
    );
    let mut ambiguous_definition = coincident_definition.clone();
    let duplicate = ambiguous_definition
        .variables
        .as_ref()
        .and_then(|table| table.points().into_iter().find(|point| point.point_id == 5))
        .expect("point 5");
    append_points(
        ambiguous_definition.variables.as_mut().expect("variables"),
        vec![duplicate],
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &ambiguous_definition
        ))
        .expect("test section solve")
        .get(&5),
        Some(&[3.0, 4.0])
    );
    let complementary = ambiguous_definition
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .iter_mut()
        .rev()
        .find(|row| row.key == 5 && row.variable_type == VariableType::U)
        .expect("duplicate point");
    complementary.value = ScalarLane::Value(3.0);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &ambiguous_definition
        ))
        .expect("test section solve")
        .get(&5),
        Some(&[3.0, 4.0])
    );
    let mut conflicting_definition = coincident_definition.clone();
    let mut conflicting = conflicting_definition
        .variables
        .as_ref()
        .and_then(|table| table.points().into_iter().find(|point| point.point_id == 2))
        .expect("point 2");
    conflicting.u = Some(30.0);
    append_points(
        conflicting_definition
            .variables
            .as_mut()
            .expect("variables"),
        vec![conflicting],
    );
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &conflicting_definition
        ))
        .expect("test section solve")
        .contains_key(&2)
    );
    let conflicting_record = crate::decode::with_test_decode_ctx(|ctx| {
        sketch_section_point_records(ctx, &conflicting_definition)
    })
    .expect("section point records admitted")
    .into_iter()
    .find(|point| point.point_id == 2)
    .expect("conflicting point record");
    let conflicting_record = serde_json::to_value(conflicting_record).expect("point record");
    assert_eq!(conflicting_record["state"], "conflicting");
    assert_eq!(
        [&conflicting_record["u"], &conflicting_record["v"]],
        [&serde_json::Value::Null; 2]
    );
    let mut point_row_type_nine = definition.clone();
    point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows[0].vertical_horizontal = Some(1));
    point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Point(
            crate::feature::definitions::FeaturePointSegment {
                point_id: 7,
                external_id: 17,
                offset: 90,
            },
        ));
    append_points(
        point_row_type_nine.variables.as_mut().expect("variables"),
        vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 7,
            u: Some(4.0),
            v: None,
        }],
    );
    *declared_solver_rows(
        &mut point_row_type_nine
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![crate::feature::definitions::FeatureSkamp {
        id: 90,
        kind: 9,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 17,
                sense: 0,
            },
        ],
        offset: 91,
    }];
    synchronize_segment_count(&mut point_row_type_nine);
    point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    synchronize_skamp_count(&mut point_row_type_nine);
    let point_row_type_nine_skamp = &point_row_type_nine
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0];
    assert_eq!(
        section_skamp_point_on_line(&point_row_type_nine, point_row_type_nine_skamp),
        Some((1, 7, crate::decode::sketch::axis::SectionAxis::V))
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_coordinates(
            ctx,
            &point_row_type_nine
        ))
        .expect("test section solve")
        .get(&7),
        Some(&[Some(4.0), Some(2.0)])
    );
}
