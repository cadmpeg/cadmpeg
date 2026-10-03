// SPDX-License-Identifier: Apache-2.0

use super::super::{declared_solver_rows, synchronize_segment_count};
use super::fixtures::base_definition;
use crate::decode::sketch::coordinates::resolved_section_points;
use crate::decode::sketch_transfer::constraints::section_dimension_constraints;
use crate::feature::definitions::test_support::append_points;
use crate::feature::definitions::{ScalarLane, VariableType};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntityId, SketchId, SketchLocus,
};

#[test]
fn section_solver_linear_dimensions_require_complete_unique_semantics() {
    let definition = base_definition();
    let mut distance_definition = super::fixtures::distance_definition(&definition);
    let mut unspanned_distance = distance_definition.clone();
    unspanned_distance
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .iter_mut()
        .find(|row| row.key == 5 && row.variable_type == VariableType::U)
        .expect("point 5")
        .value = ScalarLane::Undefined;
    unspanned_distance
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [Some(1), Some(5), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &unspanned_distance
        ))
        .expect("test section solve")
        .get(&5),
        Some(&[3.0, 2.0])
    );
    let mut disabled_distance = distance_definition.clone();
    let disabled_relations = disabled_distance.relations.as_mut().expect("relations");
    disabled_relations.skamps = Some(crate::feature::definitions::SolverSubtable::Declared {
        header: crate::feature::definitions::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        },
        rows: vec![crate::feature::definitions::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 3,
                },
            ],
            offset: 900,
        }],
    });
    disabled_relations.triples = Some(crate::feature::definitions::SolverSubtable::Declared {
        header: crate::feature::definitions::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        },
        rows: vec![crate::feature::definitions::FeatureRelationTriple {
            relation_id: Some(8),
            equation_id: None,
            skamp_id: Some(900),
            offset: 902,
        }],
    });
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &disabled_distance
        ))
        .expect("test section solve")
        .contains_key(&2)
    );
    declared_solver_rows(
        &mut distance_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .clear();
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &distance_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::VerticalDistance {
            first: SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    let mut conflicting_spanning_distance = distance_definition.clone();
    append_points(
        conflicting_spanning_distance
            .variables
            .as_mut()
            .expect("variables"),
        vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 2,
            u: Some(4.0),
            v: Some(3.0),
        }],
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &conflicting_spanning_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut conflicting_saved_spanning_distance = distance_definition.clone();
    conflicting_saved_spanning_distance.order_table =
        Some(crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 12,
                internal_id: 20,
                bitmask: 1,
                offset: 95,
            }],
            offset: 94,
        });
    conflicting_saved_spanning_distance.saved_section =
        Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
                crate::feature::definitions::FeatureSavedLine {
                    entity_id: 20,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(2.0), Some(0.0)],
                        [Some(4.0), Some(3.0), Some(0.0)],
                    ],
                    body: Vec::new(),
                    offset: 96,
                },
            )],
            offset: 96,
        });
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &conflicting_saved_spanning_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut agreeing_saved_spanning_distance = conflicting_saved_spanning_distance.clone();
    let crate::feature::definitions::FeatureSavedEntity::Line(line) =
        agreeing_saved_spanning_distance
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities
            .first_mut()
            .expect("saved line")
    else {
        panic!("saved line entity");
    };
    line.endpoints[1][0] = Some(0.0);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &agreeing_saved_spanning_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::VerticalDistance { .. }
    ));
    let mut schema_orientation = distance_definition.clone();
    let schema_dimension = &mut schema_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0];
    schema_dimension.dimension_type = 0;
    schema_dimension.value = crate::feature::definitions::DimensionValue::Resolved(0.0);
    let schema_relations = schema_orientation.relations.as_mut().expect("relations");
    let relation_id = schema_relations.rows[0].relation_id;
    schema_relations.skamps = Some(crate::feature::definitions::SolverSubtable::Declared {
        header: crate::feature::definitions::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 902,
            offset: 900,
        },
        rows: vec![crate::feature::definitions::FeatureSkamp {
            id: 900,
            kind: 2,
            flags: 0,
            status: 35,
            items: vec![crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            }],
            offset: 901,
        }],
    });
    schema_relations.triples = Some(crate::feature::definitions::SolverSubtable::Declared {
        header: crate::feature::definitions::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        },
        rows: vec![crate::feature::definitions::FeatureRelationTriple {
            relation_id: Some(relation_id),
            equation_id: None,
            skamp_id: Some(900),
            offset: 902,
        }],
    });
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &schema_orientation,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Vertical {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    schema_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value = crate::feature::definitions::DimensionValue::Resolved(1.0);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &schema_orientation,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut separate_point_distance = distance_definition.clone();
    separate_point_distance
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [Some(1), Some(5), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &separate_point_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut coincident_point_keys = separate_point_distance.clone();
    coincident_point_keys
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .iter_mut()
        .find(|row| row.key == 5 && row.variable_type == VariableType::U)
        .expect("point 5")
        .value = ScalarLane::Value(0.0);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &coincident_point_keys,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    separate_point_distance
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows.retain(|segment| !segment.point_ids().contains(&5)));
    synchronize_segment_count(&mut separate_point_distance);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &separate_point_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut incidence_oriented_distance = distance_definition.clone();
    incidence_oriented_distance
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows[0].vertical_horizontal = None);
    declared_solver_rows(
        &mut incidence_oriented_distance
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .push(crate::feature::definitions::FeatureSkamp {
        id: 18,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 0,
        }],
        offset: 83,
    });
    incidence_oriented_distance
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &incidence_oriented_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::HorizontalDistance {
            first: SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    let mut incomplete_skamps = incidence_oriented_distance.clone();
    *incomplete_skamps
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header") = crate::feature::definitions::FeatureSolverTableHeader {
        declared_count: 2,
        entity_ref: 1,
        offset: 82,
    };
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &incomplete_skamps,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut conflicting_orientation = incidence_oriented_distance.clone();
    let mut vertical = conflicting_orientation
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0]
        .clone();
    vertical.id = 19;
    vertical.kind = 2;
    vertical.offset = 84;
    declared_solver_rows(
        &mut conflicting_orientation
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .push(vertical);
    conflicting_orientation
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 2;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &conflicting_orientation,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut solver_definition = distance_definition.clone();
    declared_solver_rows(
        &mut solver_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .clear();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &solver_definition))
            .expect("test section solve")
            .get(&2),
        Some(&[0.0, 5.0])
    );
    let mut incomplete_relations = solver_definition.clone();
    incomplete_relations
        .relations
        .as_mut()
        .expect("relations")
        .declared_count = 4;
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &incomplete_relations
        ))
        .expect("test section solve")
        .contains_key(&2)
    );
    let mut equivalent_relation = solver_definition.clone();
    let duplicate = equivalent_relation
        .relations
        .as_ref()
        .expect("relations")
        .rows[0]
        .clone();
    equivalent_relation
        .relations
        .as_mut()
        .expect("relations")
        .rows
        .push(duplicate);
    equivalent_relation
        .relations
        .as_mut()
        .expect("relations")
        .declared_count = 4;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &equivalent_relation
        ))
        .expect("test section solve")
        .get(&2),
        Some(&[0.0, 5.0])
    );
    let conflicting_relation = equivalent_relation
        .relations
        .as_mut()
        .expect("relations")
        .rows
        .last_mut()
        .expect("duplicate relation");
    conflicting_relation.sign = 0xf6;
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &equivalent_relation
        ))
        .expect("test section solve")
        .contains_key(&2)
    );
    let mut duplicate_identity = solver_definition.clone();
    let mut duplicate = duplicate_identity
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0]
        .clone();
    duplicate.offset = 500;
    duplicate_identity
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            duplicate,
        ));
    synchronize_segment_count(&mut duplicate_identity);
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &duplicate_identity
        ))
        .expect("test section solve")
        .contains_key(&2)
    );
    let mut duplicate_endpoint_segment = solver_definition;
    let mut duplicate = duplicate_endpoint_segment
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0]
        .clone();
    duplicate.external_id = 99;
    duplicate.offset = 501;
    duplicate_endpoint_segment
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            duplicate,
        ));
    synchronize_segment_count(&mut duplicate_endpoint_segment);
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &duplicate_endpoint_segment
        ))
        .expect("test section solve")
        .contains_key(&2)
    );
    let mut shared_vertex_definition = distance_definition.clone();
    let mut incident = shared_vertex_definition
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[1]
        .clone();
    incident.external_id = 2;
    incident.kind = crate::feature::definitions::FeatureSegmentKind::Arc([9, 1]);
    shared_vertex_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(incident));
    synchronize_segment_count(&mut shared_vertex_definition);
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &shared_vertex_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::VerticalDistance {
            first: SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    let mut duplicate_relation_id = distance_definition.clone();
    let mut duplicate = duplicate_relation_id
        .relations
        .as_ref()
        .expect("relations")
        .rows[0]
        .clone();
    duplicate.offset = 500;
    duplicate_relation_id
        .relations
        .as_mut()
        .expect("relations")
        .rows
        .push(duplicate);
    let duplicate_constraints = crate::decode::with_test_decode_ctx(|ctx| {
        section_dimension_constraints(
            ctx,
            &duplicate_relation_id,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )
    })
    .expect("test section solve");
    assert!(duplicate_constraints
        .iter()
        .all(|(constraint, _, _)| matches!(
            constraint.definition.kind(),
            SketchConstraintDefinitionInput::Native { .. }
        )));
    assert_eq!(
        duplicate_constraints[0].0.id.as_str(),
        "creo:featdefs:sketch_constraint#917:relation:offset:80"
    );
    assert_eq!(
        duplicate_constraints[1].0.id.as_str(),
        "creo:featdefs:sketch_constraint#917:relation:offset:500"
    );
    for (constraint, _, _) in &duplicate_constraints {
        let SketchConstraintDefinitionInput::Native {
            native_properties,
            operands,
            ..
        } = constraint.definition.kind()
        else {
            panic!("duplicate relation must remain native");
        };
        assert_eq!(
            native_properties.get("relation_id").map(String::as_str),
            Some("8")
        );
        assert!(operands.iter().all(|operand| operand.field.is_some()));
    }
    let mut duplicate_measured_segment = distance_definition.clone();
    let duplicate = duplicate_measured_segment
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0]
        .clone();
    duplicate_measured_segment
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            duplicate,
        ));
    synchronize_segment_count(&mut duplicate_measured_segment);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &duplicate_measured_segment,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:0"
    ));
    duplicate_measured_segment
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            rows.last_mut().expect("duplicate").kind =
                crate::feature::definitions::FeatureSegmentKind::Line([8, 9]);
        });
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &duplicate_measured_segment,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:0"
    ));
    let mut angular_distance = distance_definition.clone();
    angular_distance
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 10;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &angular_distance, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve")[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:0"
    ));
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &angular_distance))
            .expect("test section solve")
            .contains_key(&2)
    );
    let mut duplicate_dimension = distance_definition.clone();
    let duplicate = duplicate_dimension
        .dimensions
        .as_ref()
        .expect("dimensions")
        .rows[0]
        .clone();
    duplicate_dimension
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows
        .push(duplicate);
    let duplicate_constraint = crate::decode::with_test_decode_ctx(|ctx| {
        section_dimension_constraints(
            ctx,
            &duplicate_dimension,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )
    })
    .expect("test section solve");
    assert!(matches!(
        duplicate_constraint[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Native {
            native_kind,
            parameter: None,
            operands,
            ..
        } if native_kind.as_str() == "creo:relation:0"
            && operands.first().is_some_and(|operand| operand.object_index == Some(8))
            && operands.iter().any(|operand| operand.field.as_ref().map(|field| field.name.as_str()) == Some("c[3]"))
    ));
}
