// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchId, SketchLocus};
use super::super::{declared_solver_rows, section_skamp_constraints, synchronize_segment_count, synchronize_skamp_count};
use crate::decode::sketch::coordinates::{resolved_section_coordinates, resolved_section_points};
use crate::decode::sketch::skamp::{unique_section_skamp_segment};
use super::fixtures::{base_definition, section_line_fixed_coordinate, section_skamp_saved_point_on_line};
use crate::decode::sketch_transfer::loci::{section_skamp_endpoint};
use crate::feature::definitions::test_support::{append_points};
use crate::feature::definitions::{VariableType, ScalarLane};

#[test]
fn section_solver_saved_points_preserve_incidence_symmetry_and_duplicate_refusals() {
    let definition = base_definition();
    let mut saved_definition = definition;
    saved_definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureOrderRow {
            external_id: 14,
            internal_id: 20,
            bitmask: 1,
            offset: 81,
        }],
        offset: 80,
    });
    saved_definition.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
        entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
            crate::feature::definitions::FeatureSavedLine {
                entity_id: 20,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(0.0), Some(0.0)],
                    [Some(1.0), Some(0.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 82,
            },
        )],
        offset: 82,
    });
    assert_eq!(
        crate::decode::sketch_transfer::loci::with_test_locus(|ctx, refusal| section_skamp_endpoint(
            ctx, refusal,
            &saved_definition,
            &SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture"),
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 14,
                sense: 3,
            },
        )),
        Some(SketchLocus::End(SketchEntityId::mint(
            "creo:featdefs:sketch_entity#917:14".to_string()
        ).expect("valid test fixture")))
    );
    saved_definition
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::definitions::FeatureOrderRow {
            external_id: 99,
            internal_id: 21,
            bitmask: 1,
            offset: 83,
        });
    saved_definition
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count += 1;
    saved_definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(crate::feature::definitions::FeatureSavedEntity::Line(
            crate::feature::definitions::FeatureSavedLine {
                entity_id: 21,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(1.0), Some(0.0)],
                    [Some(1.0), Some(1.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 84,
            },
        ));
    *declared_solver_rows(&mut saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 30,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        }],
        offset: 85,
    }];
    synchronize_skamp_count(&mut saved_definition);
    assert_eq!(*(section_skamp_constraints(&saved_definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition).kind(),
        SketchConstraintDefinitionInput::Horizontal {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture"),
        }
    );
    *declared_solver_rows(&mut saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 31,
        kind: 7,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
        ],
        offset: 86,
    }];
    synchronize_skamp_count(&mut saved_definition);
    let segment = unique_section_skamp_segment(&saved_definition, 12).expect("segment line");
    assert_eq!(
        section_line_fixed_coordinate(&saved_definition, segment),
        Some(crate::decode::sketch::axis::SectionAxis::V)
    );
    append_points(saved_definition.variables.as_mut().expect("variables"), vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 4,
            u: Some(3.0),
            v: None,
        }]);
    *declared_solver_rows(&mut saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 32,
        kind: 9,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 14,
                sense: 0,
            },
        ],
        offset: 87,
    }];
    synchronize_skamp_count(&mut saved_definition);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &saved_definition)).expect("test section solve").get(&4),
        Some(&[3.0, 1.0])
    );
    let mut saved_point_row_type_nine = saved_definition.clone();
    saved_point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::Point(crate::feature::definitions::FeaturePointSegment {
            point_id: 7,
            external_id: 17,
            offset: 94,
        }));
    append_points(saved_point_row_type_nine.variables.as_mut().expect("variables"), vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 7,
            u: Some(3.0),
            v: None,
        }]);
    saved_point_row_type_nine
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items[1]
        .entity_id = 17;
    synchronize_segment_count(&mut saved_point_row_type_nine);
    saved_point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    synchronize_skamp_count(&mut saved_point_row_type_nine);
    let saved_point_row_type_nine_skamp = &saved_point_row_type_nine
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0];
    assert_eq!(
        section_skamp_saved_point_on_line(
            &saved_point_row_type_nine,
            saved_point_row_type_nine_skamp,
        ),
        Some((7, crate::decode::sketch::axis::SectionAxis::V, 1.0))
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_coordinates(ctx, &saved_point_row_type_nine)).expect("test section solve").get(&7),
        Some(&[Some(3.0), Some(1.0)])
    );
    let mut saved_coincident = saved_definition.clone();
    for row in &mut saved_coincident.variables.as_mut().expect("variables").rows {
        if matches!(row.key, 4 | 5) && matches!(row.variable_type, VariableType::U | VariableType::V) {
            row.value = ScalarLane::Undefined;
        }
    }
    *declared_solver_rows(&mut saved_coincident
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![
        crate::feature::definitions::FeatureSkamp {
            id: 33,
            kind: 0,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 99,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 15,
                    sense: 2,
                },
            ],
            offset: 88,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 34,
            kind: 3,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 99,
                    sense: 3,
                },
            ],
            offset: 89,
        },
    ];
    synchronize_skamp_count(&mut saved_coincident);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &saved_coincident)).expect("test section solve"),
        BTreeMap::from([(1, [0.0, 2.0]), (4, [1.0, 1.0]), (5, [0.0, 1.0])])
    );
    saved_coincident.variables = None;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &saved_coincident)).expect("test section solve"),
        BTreeMap::from([(4, [1.0, 1.0]), (5, [0.0, 1.0])])
    );
    let mut saved_symmetric = saved_definition.clone();
    for row in saved_symmetric.variables.as_mut().expect("variables").rows.iter_mut().filter(|row| row.key == 5 && matches!(row.variable_type, VariableType::U | VariableType::V)) {
        row.value = ScalarLane::Undefined;
    }
    *declared_solver_rows(&mut saved_symmetric
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 35,
        kind: 14,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 2,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 15,
                sense: 2,
            },
        ],
        offset: 90,
    }];
    synchronize_skamp_count(&mut saved_symmetric);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &saved_symmetric)).expect("test section solve").get(&5),
        Some(&[0.0, 1.0])
    );
    saved_definition
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .iter_mut()
        .find(|row| row.key == 5 && row.variable_type == VariableType::V)
        .expect("point 5")
        .value = ScalarLane::Undefined;
    *declared_solver_rows(&mut saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 33,
        kind: 14,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 15,
                sense: 2,
            },
        ],
        offset: 88,
    }];
    synchronize_skamp_count(&mut saved_definition);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &saved_definition)).expect("test section solve").get(&5),
        Some(&[3.0, 0.0])
    );
    let mut saved_same_coordinate = saved_definition.clone();
    *declared_solver_rows(&mut saved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 36,
        kind: 17,
        flags: 1,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 2,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
        ],
        offset: 91,
    }];
    synchronize_skamp_count(&mut saved_same_coordinate);
    assert_eq!(*(section_skamp_constraints(
            &saved_same_coordinate,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::SameCoordinate { relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:99".to_string()
            ).expect("valid test fixture")),SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:12".to_string()
            ).expect("valid test fixture")),SketchCoordinateAxis::U).expect("valid test fixture") }
    );
    let mut duplicate_saved = saved_definition
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[1]
        .clone();
    if let crate::feature::definitions::FeatureSavedEntity::Line(line) = &mut duplicate_saved {
        line.offset = 86;
    }
    saved_definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(duplicate_saved);
    assert!(matches!(
        section_skamp_constraints(&saved_definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
}

