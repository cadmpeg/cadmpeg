// SPDX-License-Identifier: Apache-2.0

use super::super::{declared_solver_rows, section_skamp_constraints, synchronize_skamp_count};
use super::fixtures::base_definition;
use crate::decode::sketch::coordinates::resolved_section_points;
use crate::feature::definitions::test_support::append_points;
use crate::feature::definitions::{ScalarLane, VariableType};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchId, SketchLocus,
    SketchNativeOperand,
};

#[test]
fn section_solver_skamp_identity_and_native_state_preserve_source_semantics() {
    let definition = base_definition();
    let constraints = section_skamp_constraints(
        &definition,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    let mut equivalent_skamp = definition.clone();
    let mut redundant = equivalent_skamp
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0]
        .clone();
    redundant.id = 100;
    redundant.offset = 500;
    declared_solver_rows(
        &mut equivalent_skamp
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .push(redundant);
    let equivalent_constraints = section_skamp_constraints(
        &equivalent_skamp,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    let mut first_definition = equivalent_constraints[0].0.definition.clone().into_kind();
    let mut redundant_definition = equivalent_constraints
        .last()
        .expect("redundant")
        .0
        .definition
        .clone()
        .into_kind();
    let SketchConstraintDefinitionInput::Native {
        native_properties: first_properties,
        ..
    } = &mut first_definition
    else {
        panic!("incomplete incidence table must remain native");
    };
    let SketchConstraintDefinitionInput::Native {
        native_properties: redundant_properties,
        ..
    } = &mut redundant_definition
    else {
        panic!("incomplete incidence table must remain native");
    };
    assert_eq!(first_properties.get("id").map(String::as_str), Some("3"));
    assert_eq!(
        redundant_properties.get("id").map(String::as_str),
        Some("100")
    );
    first_properties.clear();
    redundant_properties.clear();
    assert_eq!(first_definition, redundant_definition);
    assert_ne!(
        equivalent_constraints[0].0.id,
        equivalent_constraints.last().expect("redundant").0.id
    );
    let mut duplicate_skamp_id = definition.clone();
    let mut duplicate = duplicate_skamp_id
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0]
        .clone();
    duplicate.offset = 500;
    declared_solver_rows(
        &mut duplicate_skamp_id
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .push(duplicate);
    let duplicate_constraints = section_skamp_constraints(
        &duplicate_skamp_id,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    assert!(matches!(
        duplicate_constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    assert!(matches!(
        duplicate_constraints
            .last()
            .expect("duplicate")
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    assert_eq!(
        duplicate_constraints[0].0.id.as_str(),
        "creo:featdefs:sketch_constraint#917:skamp:offset:50"
    );
    assert_eq!(
        duplicate_constraints
            .last()
            .expect("duplicate")
            .0
            .id
            .as_str(),
        "creo:featdefs:sketch_constraint#917:skamp:offset:500"
    );
    for (constraint, _) in [
        &duplicate_constraints[0],
        duplicate_constraints.last().expect("duplicate"),
    ] {
        let SketchConstraintDefinitionInput::Native {
            native_properties, ..
        } = constraint.definition.kind()
        else {
            panic!("duplicate incidence must remain native");
        };
        assert_eq!(native_properties.get("id").map(String::as_str), Some("3"));
    }
    assert_eq!(
        *(constraints[2].0.definition).kind(),
        SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_core::text::NonBlankString::new("creo:skamp:7")
                .expect("nonempty native kind"),
            native_state: Some(1),
            native_flags: Some(0),
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ],
            parameter: None,
            operands: vec![SketchNativeOperand {
                native_kind: cadmpeg_core::text::NonBlankString::new("skamp_ptr")
                    .expect("source operand kind is nonempty"),
                field: Some(cadmpeg_ir::sketches::NativeOperandField {
                    name: cadmpeg_core::text::NonBlankString::new("items.entity_id")
                        .expect("source field name is nonempty"),
                    role: Some(4)
                }),
                object_index: Some(12),
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }],
        }
    );
    let mut unavailable_skamp_entity = definition.clone();
    unavailable_skamp_entity
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[2]
        .items[0]
        .entity_id = 999;
    let unavailable_skamp_constraints = section_skamp_constraints(
        &unavailable_skamp_entity,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    assert!(matches!(
        unavailable_skamp_constraints[2].0.definition.kind(),
        SketchConstraintDefinitionInput::Native {
            entities,
            operands,
            ..
        } if entities.is_empty()
            && operands == &[SketchNativeOperand {
                native_kind: cadmpeg_core::text::NonBlankString::new("skamp_ptr").expect("source operand kind is nonempty"),
                field: Some(cadmpeg_ir::sketches::NativeOperandField { name: cadmpeg_core::text::NonBlankString::new("items.entity_id").expect("source field name is nonempty"), role: Some(4) }),
                object_index: Some(999),
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }]
    ));
    let mut stored_skamp_state = definition.clone();
    stored_skamp_state
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[2]
        .status = 34;
    stored_skamp_state
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[2]
        .flags = 0x4000;
    let stored_skamp_state = section_skamp_constraints(
        &stored_skamp_state,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    assert!(matches!(
        stored_skamp_state[2].0.definition.kind(),
        SketchConstraintDefinitionInput::Native {
            native_state: Some(34),
            native_flags: Some(0x4000),
            ..
        }
    ));
    assert_eq!(stored_skamp_state[2].0.active, Some(false));
    assert!(matches!(
        constraints[3].0.definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:skamp:1"
    ));
    assert!(matches!(
        constraints[4].0.definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:skamp:0"
    ));
    let mut center_coincidence = definition.clone();
    let center_items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 2,
        },
    ];
    center_coincidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[4]
        .items = center_items;
    assert_eq!(
        *(section_skamp_constraints(
            &center_coincidence,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[4]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                SketchLocus::Center(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                        .expect("valid test fixture")
                ),
                SketchLocus::Start(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                        .expect("valid test fixture")
                ),
            ],
        }
    );
    let mut concentric = definition.clone();
    let mut second_arc = concentric
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .find(|segment| segment.external_id == 13)
        .expect("arc")
        .clone();
    second_arc.external_id = 99;
    second_arc.offset = 501;
    concentric.segments.as_mut().expect("segments").rows.insert(
        crate::feature::segment_rows::SegmentRow::Ordinary(second_arc),
    );
    concentric
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[4]
        .items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 4,
        },
    ];
    assert_eq!(
        *(section_skamp_constraints(
            &concentric,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[4]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Concentric {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string())
                .expect("valid test fixture"),
        }
    );
    let center_relation = center_coincidence
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[4]
        .clone();
    *declared_solver_rows(
        &mut center_coincidence
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![center_relation];
    synchronize_skamp_count(&mut center_coincidence);
    let variables = center_coincidence.variables.as_mut().expect("variables");
    for row in variables.rows.iter_mut().filter(|row| {
        row.key == 1 && matches!(row.variable_type, VariableType::U | VariableType::V)
    }) {
        row.value = ScalarLane::Undefined;
    }
    append_points(
        variables,
        vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 4,
            u: Some(8.0),
            v: Some(9.0),
        }],
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &center_coincidence
        ))
        .expect("test section solve")[&1],
        [8.0, 9.0]
    );
    assert_eq!(
        *(constraints[5].0.definition).kind(),
        SketchConstraintDefinitionInput::TangentLoci {
            first: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
        }
    );
    assert_eq!(
        *(constraints[6].0.definition).kind(),
        SketchConstraintDefinitionInput::Symmetric {
            first: SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            axis: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    assert_eq!(
        *(constraints[7].0.definition).kind(),
        SketchConstraintDefinitionInput::Symmetric {
            first: SketchLocus::Center(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::Center(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            axis: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    assert_eq!(
        *(constraints[8].0.definition).kind(),
        SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                SketchLocus::Entity(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:14".to_string())
                        .expect("valid test fixture")
                ),
                SketchLocus::Center(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                        .expect("valid test fixture")
                ),
            ],
        }
    );
    assert_eq!(
        *(constraints[9].0.definition).kind(),
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Entity(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:14".to_string())
                    .expect("valid test fixture")
            ),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    let mut reversed_point_on_line = definition.clone();
    reversed_point_on_line
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[9]
        .items
        .reverse();
    assert_eq!(
        section_skamp_constraints(
            &reversed_point_on_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[9]
        .0
        .definition,
        constraints[9].0.definition
    );
    let mut line_type_three = definition.clone();
    line_type_three
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[8]
        .items[0]
        .entity_id = 12;
    assert_eq!(
        *(section_skamp_constraints(
            &line_type_three,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[8]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Center(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    let first = SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
        .expect("valid test fixture");
    let second = SketchEntityId::mint("creo:featdefs:sketch_entity#917:15".to_string())
        .expect("valid test fixture");
    assert_eq!(
        *(constraints[10].0.definition).kind(),
        SketchConstraintDefinitionInput::Perpendicular {
            first: first.clone(),
            second: second.clone(),
        }
    );
    assert_eq!(
        *(constraints[11].0.definition).kind(),
        SketchConstraintDefinitionInput::Parallel {
            first: first.clone(),
            second: second.clone(),
        }
    );
    assert_eq!(
        *(constraints[12].0.definition).kind(),
        SketchConstraintDefinitionInput::Equal { first, second }
    );
    assert_eq!(
        *(constraints[13].0.definition).kind(),
        SketchConstraintDefinitionInput::Equal {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:16".to_string())
                .expect("valid test fixture"),
        }
    );
    assert_eq!(
        *(constraints[14].0.definition).kind(),
        SketchConstraintDefinitionInput::SameCoordinate {
            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Start(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                        .expect("valid test fixture")
                ),
                SketchLocus::Start(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:15".to_string())
                        .expect("valid test fixture")
                ),
                SketchCoordinateAxis::V
            )
            .expect("valid test fixture")
        }
    );
}
