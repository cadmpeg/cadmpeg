// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::Annotations;

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn relation_program_instance_requires_the_complete_identity_frame() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(1, 1, 1, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("complete instance frame");
    assert_eq!(
        instance.framing,
        crate::native::CatiaRelationProgramInstanceFraming::Lead12
    );
    assert_eq!(instance.program_entity.entity_id, 1);
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(instance.program_entity.class_name.as_deref(), Some("body"));
    assert_eq!(instance.repeated_entity.entity_id, 1);
    assert_eq!(
        instance.repeated_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(instance.repeated_entity.class_name.as_deref(), Some("body"));
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.reference.entity_id)
            .collect::<Vec<_>>(),
        [20, 21, 23, 25, 1, 1, 21, 27]
    );
    assert_eq!(
        instance.reference_incidences[4]
            .reference
            .class_name
            .as_deref(),
        Some("body")
    );
    assert_eq!(
        instance.reference_incidences[5]
            .reference
            .class_name
            .as_deref(),
        Some("body")
    );
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.payload_offset)
            .collect::<Vec<_>>(),
        [0, 10, 40, 50, 60, 65, 75, 85]
    );
    assert_eq!(
        instance.relation_expression.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance
            .parameter_dependencies
            .iter()
            .map(|dependency| dependency.symbol.as_str())
            .collect::<Vec<_>>(),
        ["#1_", "#2_", "#2_"]
    );
    assert_eq!(
        instance
            .parameter_dependencies
            .iter()
            .map(|dependency| dependency.source_offset)
            .collect::<Vec<_>>(),
        [19, 23, 28]
    );
    assert!(instance
        .parameter_dependencies
        .iter()
        .all(|dependency| dependency.candidates.is_empty()));
    assert!(instance.inputs.is_none());
    let context = instance
        .lead12_context_entity
        .as_ref()
        .expect("lead-12 context entity");
    assert_eq!(context.entity_id, 1);
    assert_eq!(
        context.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(context.class_name.as_deref(), Some("body"));
    assert!(instance.output_entity.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(2, 1, 3, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("resolved non-expression program");
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert!(instance.relation_expression.is_none());
    let context = instance
        .lead12_context_entity
        .as_ref()
        .expect("lead-12 context entity");
    assert_eq!(context.entity_id, 3);
    assert!(context.entity.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(3, 3, 1, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("unresolved program identity");
    assert!(instance.program_entity.entity.is_none());
    assert_eq!(instance.repeated_entity.entity_id, 3);
    assert!(instance.repeated_entity.entity.is_none());
    assert!(instance.relation_expression.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(1, 1, 1, 3),
    );
    assert!(native
        .entity_records
        .iter()
        .all(|entity| entity.relation_program_instance.is_none()));
}

#[test]
fn relation_program_output_selects_only_the_framing_specific_paramout_slot() {
    let lead12 = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance_class(1, 1, 1, 2, "paramout"),
    );
    let lead12_instance = lead12.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-12 relation-program instance");
    assert_eq!(
        lead12_instance.output_entity,
        lead12_instance.lead12_context_entity
    );
    assert_eq!(
        lead12_instance
            .output_entity
            .as_ref()
            .and_then(|output| output.class_name.as_deref()),
        Some("paramout")
    );
    assert!(lead12_instance.lead54_trailing_entity.is_none());
    let lead12_decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_relation_program_instance_class(
                1, 1, 1, 2, "paramout",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode lead-12 paramout relation-program instance");
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_RELATION_PROGRAM_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_RELATION_PROGRAM_OUTPUT_COUNT),
        0
    );

    let lead12_body = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance_class(1, 1, 1, 2, "body"),
    );
    assert!(lead12_body.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-12 body relation-program instance")
        .output_entity
        .is_none());

    let lead54 = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance_class(1, 1, 1, 2, "paramout"),
    );
    let lead54_instance = lead54.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-54 relation-program instance");
    assert_eq!(
        lead54_instance.output_entity,
        lead54_instance.lead54_trailing_entity
    );
    assert_eq!(
        lead54_instance
            .output_entity
            .as_ref()
            .and_then(|output| output.class_name.as_deref()),
        Some("paramout")
    );
    assert!(lead54_instance.lead12_context_entity.is_none());

    let lead54_body = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance_class(1, 1, 1, 2, "body"),
    );
    assert!(lead54_body.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-54 body relation-program instance")
        .output_entity
        .is_none());
}

#[test]
fn relation_program_inputs_require_complete_unique_signature_bindings() {
    let signature = crate::native::CatiaRelationTypeSignature {
        inputs: vec![
            crate::native::CatiaRelationTypeInput {
                parameter: "#1_".to_string(),
                input_type: "LENGTH".to_string(),
            },
            crate::native::CatiaRelationTypeInput {
                parameter: "#2_".to_string(),
                input_type: "Real".to_string(),
            },
        ],
        result_type: "Real".to_string(),
    };
    let reference = |entity_id: u32| crate::native::CatiaEntityReference {
        entity_id,
        is_null: false,
        entity: Some(format!("entity-{entity_id}")),
        class_name: Some("param".to_string()),
    };
    let dependency =
        |symbol: &str, candidates: Vec<_>| crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: symbol.to_string(),
            candidates,
        };
    let complete = vec![
        dependency("#1_", vec![reference(10)]),
        dependency("#1_ /2", vec![reference(10)]),
        dependency("#2_", vec![reference(11)]),
    ];
    let inputs = crate::native::resolved_relation_program_inputs(&signature, &complete)
        .expect("complete ordered input bindings");
    assert_eq!(
        inputs
            .iter()
            .map(|input| (input.parameter.as_str(), input.entity.entity_id))
            .collect::<Vec<_>>(),
        [("#1_", 10), ("#2_", 11)]
    );

    let compact_ordinal = vec![
        dependency("#1_", vec![reference(10)]),
        dependency("#1_/2", vec![reference(10)]),
        dependency("#2_", vec![reference(11)]),
    ];
    assert!(
        crate::native::resolved_relation_program_inputs(&signature, &compact_ordinal).is_some()
    );

    let zero = crate::native::CatiaRelationTypeSignature {
        inputs: Vec::new(),
        result_type: "Real".to_string(),
    };
    assert_eq!(
        crate::native::resolved_relation_program_inputs(&zero, &[]),
        Some(Vec::new())
    );
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[dependency("#1_", vec![reference(10)])]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#2_", vec![reference(10)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#1_ /2", vec![reference(12)]),
            dependency("#2_", vec![reference(11)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#1_ /ordinal", vec![reference(10)]),
            dependency("#2_", vec![reference(11)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10), reference(12)]),
            dependency("#2_", vec![reference(11)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#2_", vec![reference(11)]),
            dependency("#3_", vec![reference(12)])
        ]
    )
    .is_none());
}

#[test]
fn complete_relation_program_inputs_transfer_typed_parameters() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let parameter_entity = native.entity_records[2].clone();
    native.entity_records[0].formula_relation = None;
    native.entity_records[0].relation_program_instance =
        Some(crate::native::CatiaRelationProgramInstance {
            framing: crate::native::CatiaRelationProgramInstanceFraming::Lead12,
            program_entity: crate::native::CatiaEntityReference::default(),
            repeated_entity: crate::native::CatiaEntityReference::default(),
            reference_incidences: Vec::new(),
            relation_expression: None,
            parameter_dependencies: Vec::new(),
            inputs: Some(vec![crate::native::CatiaRelationProgramInput {
                parameter: "#1_".to_string(),
                value_type: "LENGTH".to_string(),
                entity: crate::native::CatiaEntityReference {
                    entity_id: parameter_entity.entity_id,
                    is_null: false,
                    entity: Some(parameter_entity.id.clone()),
                    class_name: Some("param".to_string()),
                },
            }]),
            output_entity: None,
            lead12_context_entity: None,
            lead54_trailing_entity: None,
        });

    let mut ir = CadIr::empty();
    let mut annotations = Annotations::default();
    let transfer = crate::formula::transfer_parameters(&mut ir, &native, &mut annotations, None);
    let [parameter] = ir.model.parameters.as_slice() else {
        panic!("one relation-program input parameter")
    };
    assert_eq!(transfer.relation_program_parameter_count, 1);
    assert_eq!(parameter.name, "Thickness");
    assert_eq!(parameter.expression, "35 mm");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(
        parameter.properties.get("value_type").map(String::as_str),
        Some("LENGTH")
    );
    assert_eq!(
        parameter
            .properties
            .get("catia_binding")
            .map(String::as_str),
        Some("#1_ /2")
    );
    assert_eq!(parameter.native_ref, Some(parameter_entity.id.clone()));

    let mut empty_binding_native = native.clone();
    empty_binding_native.entity_records[2]
        .parameter_value
        .as_mut()
        .expect("complete input parameter")
        .binding
        .value
        .clear();
    let mut empty_binding_ir = CadIr::empty();
    let empty_binding_transfer = crate::formula::transfer_parameters(
        &mut empty_binding_ir,
        &empty_binding_native,
        &mut Annotations::default(),
        None,
    );
    let [empty_binding_parameter] = empty_binding_ir.model.parameters.as_slice() else {
        panic!("one empty-binding input parameter")
    };
    assert_eq!(empty_binding_transfer.relation_program_parameter_count, 1);
    assert_eq!(
        empty_binding_parameter
            .properties
            .get("catia_binding")
            .map(String::as_str),
        Some("")
    );

    let mut conflicting_native = native.clone();
    let mut conflicting_instance = conflicting_native.entity_records[0]
        .relation_program_instance
        .clone()
        .expect("complete relation-program instance");
    conflicting_instance
        .inputs
        .as_mut()
        .expect("complete relation-program inputs")[0]
        .value_type = "Real".to_string();
    conflicting_native.entity_records[1].relation_program_instance = Some(conflicting_instance);
    let mut conflicting_ir = CadIr::empty();
    let conflicting_transfer = crate::formula::transfer_parameters(
        &mut conflicting_ir,
        &conflicting_native,
        &mut Annotations::default(),
        None,
    );
    assert_eq!(conflicting_transfer.relation_program_parameter_count, 0);
    assert!(conflicting_ir.model.parameters.is_empty());
}

#[test]
fn complete_relation_program_output_transfers_a_typed_result() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let expression_entity = native.entity_records[1].clone();
    let input_entity = native.entity_records[2].clone();
    let output_entity = native.entity_records[3].clone();
    native.entity_records[0].formula_relation = None;
    native.entity_records[0].relation_program_instance =
        Some(crate::native::CatiaRelationProgramInstance {
            framing: crate::native::CatiaRelationProgramInstanceFraming::Lead12,
            program_entity: crate::native::CatiaEntityReference::default(),
            repeated_entity: crate::native::CatiaEntityReference::default(),
            reference_incidences: Vec::new(),
            relation_expression: Some(expression_entity.id.clone()),
            parameter_dependencies: Vec::new(),
            inputs: Some(vec![crate::native::CatiaRelationProgramInput {
                parameter: "#1_".to_string(),
                value_type: "LENGTH".to_string(),
                entity: crate::native::CatiaEntityReference {
                    entity_id: input_entity.entity_id,
                    is_null: false,
                    entity: Some(input_entity.id.clone()),
                    class_name: Some("param".to_string()),
                },
            }]),
            output_entity: Some(crate::native::CatiaEntityReference {
                entity_id: output_entity.entity_id,
                is_null: false,
                entity: Some(output_entity.id.clone()),
                class_name: Some("paramout".to_string()),
            }),
            lead12_context_entity: None,
            lead54_trailing_entity: None,
        });

    let mut ir = CadIr::empty();
    let mut annotations = Annotations::default();
    let transfer = crate::formula::transfer_parameters(&mut ir, &native, &mut annotations, None);
    let [input, output] = ir.model.parameters.as_slice() else {
        panic!("typed relation-program input and output")
    };
    assert_eq!(transfer.relation_program_parameter_count, 1);
    assert_eq!(input.name, "Thickness");
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(output.name, "Result");
    assert_eq!(output.expression, "#1_ /2-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.properties["catia_binding"], "#result_ /1");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.native_ref, Some(output_entity.id));

    let mut ambiguous_native = native;
    let duplicate_program = ambiguous_native.entity_records[0]
        .relation_program_instance
        .clone()
        .expect("compound relation-program instance");
    ambiguous_native.entity_records[1].relation_program_instance = Some(duplicate_program);
    let mut ambiguous_ir = CadIr::empty();
    let ambiguous_transfer = crate::formula::transfer_parameters(
        &mut ambiguous_ir,
        &ambiguous_native,
        &mut Annotations::default(),
        None,
    );
    let [ambiguous_input] = ambiguous_ir.model.parameters.as_slice() else {
        panic!("ambiguous compound output keeps its typed input")
    };
    assert_eq!(ambiguous_transfer.relation_program_parameter_count, 1);
    assert_eq!(ambiguous_input.name, "Thickness");
}

#[test]
fn lead54_relation_program_instance_requires_its_complete_identity_frame() {
    let file = standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 2);
    let native = crate::native::CatiaNative::decode(&file);
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("complete lead-54 instance frame");
    assert_eq!(
        instance.framing,
        crate::native::CatiaRelationProgramInstanceFraming::Lead54
    );
    assert!(instance.lead12_context_entity.is_none());
    let trailing = instance
        .lead54_trailing_entity
        .as_ref()
        .expect("lead-54 trailing entity");
    assert_eq!(trailing.entity_id, 1);
    assert_eq!(
        trailing.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(trailing.class_name.as_deref(), Some("body"));
    assert!(instance.output_entity.is_none());
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance.repeated_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.reference.entity_id)
            .collect::<Vec<_>>(),
        [5, 20, 1, 21, 5]
    );
    assert_eq!(
        instance.reference_incidences[2]
            .reference
            .class_name
            .as_deref(),
        Some("body")
    );
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.payload_offset)
            .collect::<Vec<_>>(),
        [10, 35, 55, 60, 70]
    );
    assert_eq!(
        instance.relation_expression.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance
            .parameter_dependencies
            .iter()
            .map(|dependency| dependency.symbol.as_str())
            .collect::<Vec<_>>(),
        ["#1_", "#2_", "#2_"]
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode lead-54 relation-program instance");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT),
        3
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT
        ),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_TYPED_RELATION_PROGRAM_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DISTINCT_RELATION_PROGRAM_INPUT_ENTITY_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_RELATION_PROGRAM_INPUT_PARAMETER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT
        ),
        0
    );

    let unresolved = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance(1, 3, 3, 2),
    );
    let instance = unresolved.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("unresolved repeated identity");
    assert_eq!(instance.repeated_entity.entity_id, 3);
    assert!(instance.repeated_entity.entity.is_none());
    let trailing = instance
        .lead54_trailing_entity
        .as_ref()
        .expect("lead-54 trailing entity");
    assert_eq!(trailing.entity_id, 3);
    assert!(trailing.entity.is_none());

    let malformed = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 3),
    );
    assert!(malformed
        .entity_records
        .iter()
        .all(|entity| entity.relation_program_instance.is_none()));
}

#[test]
fn decode_reports_exact_relation_program_instances() {
    for (
        program_entity_id,
        repeated_reference_entity_id,
        resolved,
        expression,
        other,
        unresolved,
        resolved_repeated,
    ) in [
        (1, 1, 1, 1, 0, 0, 1),
        (2, 1, 1, 0, 1, 0, 1),
        (3, 1, 0, 0, 0, 0, 1),
        (1, 3, 1, 1, 0, 0, 0),
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_relation_program_instance(
                    program_entity_id,
                    repeated_reference_entity_id,
                    1,
                    2,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode relation-program instance");
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_INSTANCE_COUNT),
            1
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            8
        );
        let resolved_reference_incidences = 1 + usize::from(repeated_reference_entity_id == 1);
        let null_reference_incidences = usize::from(repeated_reference_entity_id == 3);
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            resolved_reference_incidences
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_NULL_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            null_reference_incidences
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            8 - resolved_reference_incidences - null_reference_incidences
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            resolved_reference_incidences
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT),
            1
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
            ),
            1
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
            ),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT
            ),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT
            ),
            1
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
            ),
            0
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INSTANCE_COUNT),
            resolved
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RELATION_EXPRESSION_PROGRAM_INSTANCE_COUNT
            ),
            expression
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_OTHER_RELATION_PROGRAM_INSTANCE_COUNT),
            other
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::UNRESOLVED_RELATION_PROGRAM_INSTANCE_COUNT),
            unresolved,
            "program entity {program_entity_id}"
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_NULL_RELATION_PROGRAM_INSTANCE_COUNT),
            usize::from(program_entity_id == 3)
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT
            ),
            resolved_repeated
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT
            ),
            usize::from(repeated_reference_entity_id > 3)
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_NULL_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT
            ),
            usize::from(repeated_reference_entity_id == 3)
        );
        let classified_program = usize::from(program_entity_id <= 2);
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT),
            classified_program
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT),
            1 - classified_program
        );
        let classified_repeated = usize::from(repeated_reference_entity_id == 1);
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT
            ),
            classified_repeated
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT
            ),
            1 - classified_repeated
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_INSTANCED_RELATION_EXPRESSION_COUNT),
            expression
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
            expression
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT
            ),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT
            ),
            expression
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
            1 - expression
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT
            ),
            expression * 3
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT
            ),
            expression * 3
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
            0
        );
        assert!(decoded.ir().model.parameters.is_empty());
    }
}

#[test]
fn native_load_derives_relation_program_instances_from_older_namespaces() {
    for native in [
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_program_instance(
            1, 1, 1, 2,
        )),
        crate::native::CatiaNative::decode(
            &standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 2),
        ),
    ] {
        let expected = native.entity_records[1]
            .relation_program_instance
            .clone()
            .expect("decoded relation-program instance");
        let mut stored = cadmpeg_ir::NativeNamespace::default();
        native
            .store(&mut stored)
            .expect("store older relation-program namespace");
        for (version, remove_context, remove_trailing, remove_framing) in [
            (
                crate::native::CATIA_RELATION_PROGRAM_INPUT_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_TYPED_REFERENCE_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_CONTEXT_VERSION - 1,
                true,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_CONTEXT_VERSION - 2,
                true,
                true,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_INSTANCE_VERSION,
                true,
                true,
                true,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_INSTANCE_VERSION - 1,
                true,
                true,
                true,
            ),
        ] {
            let mut namespace = stored.clone();
            namespace.set_version(std::num::NonZeroU32::new(version).unwrap());
            let mut stored_fields = namespace
                .arenas
                .get_mut("entity_records")
                .expect("stored entity records")[1]
                .fields_mut();
            let stored_instance = stored_fields
                .get_mut("relation_program_instance")
                .expect("stored relation-program field")
                .as_object_mut()
                .expect("stored relation-program instance");
            if remove_context {
                stored_instance.remove("lead12_context_entity");
            }
            if remove_trailing {
                stored_instance.remove("lead54_trailing_entity");
            }
            if remove_framing {
                stored_instance.remove("framing");
            }
            stored_instance.remove("output_entity");
            stored_instance.remove("inputs");
            stored_instance.remove("reference_incidences");
            stored_instance.remove("parameter_dependencies");
            stored_instance.remove("program_entity");
            stored_instance.remove("repeated_entity");
            for field in ["lead12_context_entity", "lead54_trailing_entity"] {
                if let Some(reference) = stored_instance
                    .get_mut(field)
                    .and_then(|value| value.as_object_mut())
                {
                    reference.remove("class_name");
                }
            }

            drop(stored_fields);
            let migrated = crate::native::CatiaNative::load(&namespace)
                .expect("migrate relation-program instance");
            assert_eq!(
                migrated.entity_records[1]
                    .relation_program_instance
                    .as_ref(),
                Some(&expected)
            );
        }

        let mut namespace = stored.clone();
        namespace.set_version(
            std::num::NonZeroU32::new(crate::native::CATIA_RELATION_REFERENCE_OFFSET_VERSION - 1)
                .unwrap(),
        );
        let mut stored_fields = namespace
            .arenas
            .get_mut("entity_records")
            .expect("stored entity records")[1]
            .fields_mut();
        let incidences = stored_fields
            .get_mut("relation_program_instance")
            .expect("stored relation-program field")
            .as_object_mut()
            .expect("stored relation-program instance")
            .get_mut("reference_incidences")
            .expect("stored reference incidences")
            .as_array_mut()
            .expect("stored reference incidences");
        for incidence in incidences {
            *incidence =
                incidence.as_object().expect("stored reference incidence")["reference"].clone();
        }
        drop(stored_fields);
        let migrated = crate::native::CatiaNative::load(&namespace)
            .expect("migrate relation-program reference offsets");
        assert_eq!(
            migrated.entity_records[1]
                .relation_program_instance
                .as_ref(),
            Some(&expected)
        );

        let mut namespace = stored.clone();
        namespace.set_version(
            std::num::NonZeroU32::new(crate::native::CATIA_RELATION_DEPENDENCY_OFFSET_VERSION - 1)
                .unwrap(),
        );
        let mut stored_fields = namespace
            .arenas
            .get_mut("entity_records")
            .expect("stored entity records")[1]
            .fields_mut();
        let dependencies = stored_fields
            .get_mut("relation_program_instance")
            .expect("stored relation-program field")
            .as_object_mut()
            .expect("stored relation-program instance")
            .get_mut("parameter_dependencies")
            .expect("stored parameter dependencies")
            .as_array_mut()
            .expect("stored parameter dependencies");
        for dependency in dependencies {
            dependency
                .as_object_mut()
                .expect("stored parameter dependency")
                .remove("source_offset");
        }
        drop(stored_fields);
        let migrated = crate::native::CatiaNative::load(&namespace)
            .expect("migrate relation-program dependency offsets");
        assert_eq!(
            migrated.entity_records[1]
                .relation_program_instance
                .as_ref(),
            Some(&expected)
        );

        let mut malformed_dependencies = native.clone();
        malformed_dependencies.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .parameter_dependencies[0]
            .symbol = "#999_".to_string();
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed_dependencies
            .store(&mut namespace)
            .expect("store malformed relation-program dependencies");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));

        let mut malformed_inputs = native.clone();
        malformed_inputs.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .inputs = Some(Vec::new());
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed_inputs
            .store(&mut namespace)
            .expect("store malformed relation-program inputs");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));

        let mut malformed_offset = native.clone();
        malformed_offset.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .reference_incidences[0]
            .payload_offset = u64::MAX;
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed_offset
            .store(&mut namespace)
            .expect("store malformed relation-program incidence offset");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));

        let mut malformed = native;
        malformed.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .reference_incidences[0]
            .reference
            .entity_id = u32::MAX;
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed relation-program incidences");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }
}

#[test]
fn native_load_rederives_relation_program_paramout_outputs_from_older_namespaces() {
    for native in [
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_program_instance_class(
            1, 1, 1, 2, "paramout",
        )),
        crate::native::CatiaNative::decode(
            &standard_catpart_with_lead54_relation_program_instance_class(1, 1, 1, 2, "paramout"),
        ),
    ] {
        let expected = native.entity_records[1]
            .relation_program_instance
            .clone()
            .expect("decoded paramout relation-program instance");
        assert!(expected.output_entity.is_some());
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        native
            .store(&mut namespace)
            .expect("store paramout relation-program instance");
        namespace.set_version(
            std::num::NonZeroU32::new(crate::native::CATIA_RELATION_PROGRAM_OUTPUT_VERSION - 1)
                .unwrap(),
        );
        namespace
            .arenas
            .get_mut("entity_records")
            .expect("stored entity records")[1]
            .fields_mut()
            .get_mut("relation_program_instance")
            .expect("stored relation-program field")
            .as_object_mut()
            .expect("stored relation-program instance")
            .remove("output_entity");

        let migrated = crate::native::CatiaNative::load(&namespace)
            .expect("migrate paramout relation-program output");
        assert_eq!(
            migrated.entity_records[1]
                .relation_program_instance
                .as_ref(),
            Some(&expected)
        );
    }
}
