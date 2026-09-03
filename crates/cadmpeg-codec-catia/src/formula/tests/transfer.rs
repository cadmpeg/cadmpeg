// SPDX-License-Identifier: Apache-2.0
//! Formula transfer tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn decode_transfers_a_complete_typed_input_when_the_formula_output_is_unresolved() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_relation(0x63, false)),
            &DecodeOptions::default(),
        )
        .expect("decode formula with unresolved output");
    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("independently typed formula input")
    };

    assert_eq!(input.name, "Thickness");
    assert_eq!(input.ordinal, 0);
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert!(input.dependencies.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_FORMULA_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT),
        1
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn formula_input_with_additional_object_payload_remains_unresolved() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    0x63,
                    false,
                    &[("#1_", "LENGTH", "Thickness", "#1_ /2", 35.0)],
                    "LENGTH",
                    Some(33.0),
                    "#1_ /2-2mm",
                    (&[0x81, 0xfe], None),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode formula input with additional object payload");

    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        4
    );
}

#[test]
fn decode_transfers_a_closed_length_formula_and_its_input() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let bytes = standard_catpart_with_formula_relation(4, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let output_entity = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .output_entity
        .reference;
    assert_eq!(output_entity.entity_id, 4);
    assert!(output_entity.entity.is_some());
    assert_eq!(
        output_entity.class_name,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id == Some(4))
            .and_then(|record| record.class_name.clone())
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode closed length formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("closed formula parameters")
    };

    assert_eq!(input.name, "Thickness");
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(input.properties["value_type"], "LENGTH");
    assert_eq!(input.properties["catia_binding"], "#1_ /2");
    assert!(input.dependencies.is_empty());
    assert_eq!(output.name, "Result");
    assert_eq!(output.ordinal, 1);
    assert_eq!(output.expression, "#1_ /2-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.properties["catia_binding"], "#result_ /1");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        4
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_FORMULA_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        usize::from(output_entity.class_name.is_some())
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        usize::from(output_entity.class_name.is_none())
    );
    let expression_classified = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .expression_entity
        .reference
        .class_name
        .is_some();
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT),
        usize::from(expression_classified)
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT),
        usize::from(!expression_classified)
    );
    let dependency_candidate = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0]
        .candidates[0];
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT
        ),
        usize::from(dependency_candidate.class_name.is_some())
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNCLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT
        ),
        usize::from(dependency_candidate.class_name.is_none())
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().all(|loss| {
        loss.code.category() != cadmpeg_ir::report::LossCategory::DesignIntent
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    assert_eq!(
        decoded.source_fidelity().annotations.exactness[&input.id.0].fields["expression"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        decoded.source_fidelity().annotations.exactness[&input.id.0].fields["properties"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        decoded.source_fidelity().annotations.exactness[&output.id.0].fields["properties"],
        cadmpeg_ir::Exactness::Derived
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_keeps_a_mismatched_formula_result_unresolved() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 35.0)],
                "LENGTH",
                Some(34.0),
                "#1_ /2-2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula with mismatched stored result");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
    assert!(input.dependencies.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        3
    );
}

#[test]
fn decode_transfers_a_closed_constant_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(12.0),
                "10mm+2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode constant formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("constant formula output")
    };
    assert_eq!(output.name, "Result");
    assert_eq!(output.expression, "10mm+2mm");
    assert!(output.dependencies.is_empty());
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.0)
        ))
    );
    assert!(decoded
        .source_fidelity()
        .annotations
        .exactness
        .get(&output.id.0)
        .is_none_or(|annotation| !annotation.fields.contains_key("expression")));
}

#[test]
fn decode_transfers_linear_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode linear interpolation formula");

    let [start, end, fraction, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("linear interpolation parameters")
    };
    assert_eq!(start.value, Some(cadmpeg_ir::ParameterValue::Real(2.0)));
    assert_eq!(end.value, Some(cadmpeg_ir::ParameterValue::Real(10.0)));
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(output.value, Some(cadmpeg_ir::ParameterValue::Real(4.0)));
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_transfers_cubic_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(3.25),
                "CubicInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode cubic interpolation formula");

    let [start, end, fraction, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("cubic interpolation parameters")
    };
    assert_eq!(start.value, Some(cadmpeg_ir::ParameterValue::Real(2.0)));
    assert_eq!(end.value, Some(cadmpeg_ir::ParameterValue::Real(10.0)));
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(output.value, Some(cadmpeg_ir::ParameterValue::Real(3.25)));
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_transfers_dimensioned_linear_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Start", "#1_ /2", 2.0),
                    ("#2_", "LENGTH", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "LENGTH",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned linear interpolation formula");

    let [start, end, fraction, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("dimensioned linear interpolation parameters")
    };
    assert_eq!(
        start.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
    assert_eq!(
        end.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(10.0)
        ))
    );
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(4.0)
        ))
    );
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_transfers_typed_integer_to_angle_formula() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "Integer",
                "ANGLE",
                2.0,
                0.5,
                "#1_ /2*0.25rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode typed formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("typed formula parameters")
    };

    assert_eq!(input.expression, "2");
    assert_eq!(input.value, Some(ParameterValue::Integer(2)));
    assert_eq!(output.value, Some(ParameterValue::Angle(Angle(0.5))));
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_transfers_dimensionless_real_formula() {
    use cadmpeg_ir::features::ParameterValue;

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4, false, "Real", "R", 2.5, 1.25, "#1_ /2/2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode real formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("real formula parameters")
    };

    assert_eq!(input.expression, "2.5");
    assert_eq!(input.value, Some(ParameterValue::Real(2.5)));
    assert_eq!(input.properties["value_type"], "Real");
    assert_eq!(output.value, Some(ParameterValue::Real(1.25)));
    assert_eq!(output.properties["value_type"], "Real");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    for parameter in [input, output] {
        assert_eq!(
            decoded.source_fidelity().annotations.exactness[&parameter.id.0].fields["properties"],
            cadmpeg_ir::Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_an_unset_typed_formula_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                None,
                "#1_ /2+1mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset formula result");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("unset formula parameters")
    };

    assert_eq!(output.value, None);
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.expression, "#1_ /2+1mm");
    assert_eq!(output.properties["value_type"], "LENGTH");
}

#[test]
fn decode_transfers_a_typed_boolean_predicate_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "X", "#1_ /2", 5.0),
                    ("#2_", "Real", "Y", "#2_ /2", 3.0),
                ],
                "Boolean",
                None,
                "(#1_ /2>#2_ /2) and (#1_ /2>=0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean predicate formula");
    let [x, y, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("predicate formula parameters")
    };

    assert_eq!(output.value, None);
    assert_eq!(output.properties["value_type"], "Boolean");
    assert_eq!(output.expression, "(#1_ /2>#2_ /2) and (#1_ /2>=0)");
    assert_eq!(output.dependencies, [x.id.clone(), y.id.clone()]);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        5
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_an_unset_typed_formula_input_as_an_unset_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    4,
                    false,
                    &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                    "LENGTH",
                    None,
                    "#1_ /2+1mm",
                    (&[0xfe], Some(0)),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unset formula input");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("unset formula parameters")
    };

    assert_eq!(input.name, "Width");
    assert_eq!(input.value, None);
    assert!(input.expression.is_empty());
    assert!(input.dependencies.is_empty());
    assert_eq!(input.properties["value_type"], "LENGTH");
    assert_eq!(input.properties["catia_binding"], "#1_ /2");
    assert_eq!(output.value, None);
    assert_eq!(output.expression, "#1_ /2+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.properties["catia_binding"], "#result_ /1");
}

#[test]
fn decode_transfers_unset_non_numeric_formula_inputs_without_deriving_the_output() {
    for parameter_type in ["Boolean", "String"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(
                    standard_catpart_with_typed_formula_inputs_and_object_payload(
                        4,
                        false,
                        &[("#1_", parameter_type, "Value", "#1_ /2", 1.0)],
                        "Real",
                        Some(1.0),
                        "#1_ /2",
                        (&[0xfe], Some(0)),
                    ),
                ),
                &DecodeOptions::default(),
            )
            .expect("decode unset non-numeric formula input");
        let [input] = decoded.ir().model.parameters.as_slice() else {
            panic!("only the independently typed unset input")
        };

        assert_eq!(input.name, "Value");
        assert_eq!(input.value, None);
        assert!(input.expression.is_empty());
        assert!(input.dependencies.is_empty());
        assert_eq!(input.properties["value_type"], parameter_type);
        assert_eq!(input.properties["catia_binding"], "#1_ /2");
        assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn decode_transfers_an_unset_string_formula_result_without_evaluation() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    4,
                    false,
                    &[("#1_", "String", "Value", "#1_", 1.0)],
                    "String",
                    None,
                    "#1_",
                    (&[0xfe], Some(0)),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unset String formula result");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("unset String formula parameters")
    };

    assert_eq!(input.value, None);
    assert_eq!(input.properties["value_type"], "String");
    assert_eq!(output.value, None);
    assert_eq!(output.expression, "#1_");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.properties["value_type"], "String");
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_does_not_treat_numeric_packets_as_non_numeric_formula_values() {
    for parameter_type in ["Boolean", "String"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    4,
                    false,
                    &[("#1_", parameter_type, "Value", "#1_ /2", 1.0)],
                    "Real",
                    Some(1.0),
                    "#1_ /2",
                )),
                &DecodeOptions::default(),
            )
            .expect("decode non-numeric formula input with numeric packet");

        assert!(decoded.ir().model.parameters.is_empty());
    }
}

#[test]
fn decode_rejects_nonintegral_integer_formula_input() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "Integer",
                "I",
                3.5,
                4.0,
                "#1_ /2-2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode invalid integer formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_deduplicates_repeated_single_input_formula_symbols() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "ANGLE",
                "ANGLE",
                0.25,
                0.5,
                "#1_ /2+#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode repeated formula input");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("repeated formula input parameters")
    };

    assert_eq!(input.expression, "0.25 rad");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
}

#[test]
fn decode_transfers_ordered_multi_input_formula_dependencies() {
    use cadmpeg_ir::features::ParameterValue;

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "Width", "#1_ /2", 12.0),
                    ("#2_", "Integer", "Count", "#2_ /3", 3.0),
                ],
                "Real",
                Some(15.0),
                "#1_ /2+#2_ /3",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode multi-input formula");
    let [width, count, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("multi-input formula parameters")
    };

    assert_eq!(width.value, Some(ParameterValue::Real(12.0)));
    assert_eq!([width.ordinal, count.ordinal, output.ordinal], [0, 1, 2]);
    assert_eq!(count.value, Some(ParameterValue::Integer(3)));
    assert_eq!(
        output.dependencies,
        [width.id.clone(), count.id.clone()].as_slice()
    );
    assert_eq!(output.value, Some(ParameterValue::Real(15.0)));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_transfers_a_closed_formula_with_bare_symbols() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let bytes = standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#1_", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_-2mm",
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("decode bare-symbol formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("closed bare-symbol formula parameters")
    };

    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(output.expression, "#1_-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));

    let native = crate::native::CatiaNative::decode(&bytes);
    let mut excluded_ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::Annotations::default();
    let excluded = crate::formula::transfer_parameters(
        &mut excluded_ir,
        &native,
        &mut annotations,
        Some(&std::collections::HashSet::new()),
    );
    assert!(excluded_ir.model.parameters.is_empty());
    assert!(excluded.consumed_object_records.is_empty());
}

#[test]
fn decode_transfers_each_supported_formula_input_independently() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Width", "#1_ /2", 12.0),
                    ("#2_", "String", "Label", "#2_ /3", 0.25),
                    ("#3_", "Real", "Depth", "#3_ /4", 6.5),
                ],
                "Real",
                Some(3.0),
                "#1_ /2+#2_ /3+#3_ /4",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode incomplete multi-input formula");

    let [width, depth] = decoded.ir().model.parameters.as_slice() else {
        panic!("independently bound formula inputs")
    };
    assert_eq!(width.name, "Width");
    assert_eq!(
        width.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.0)
        ))
    );
    assert!(width.dependencies.is_empty());
    assert_eq!(depth.name, "Depth");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(6.5))
    );
    assert!(depth.dependencies.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        4
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("4 modeling-scope field record(s)")
    }));
}

#[test]
fn decode_transfers_a_chained_formula_definition_once() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::Linear,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula chain");
    let [input, intermediate, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("formula chain parameters")
    };

    assert_eq!(intermediate.expression, "#1_ /2+1mm");
    assert_eq!(intermediate.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.expression, "#2_ /3+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&intermediate.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_rejects_multiple_formula_definitions_for_one_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::DuplicateTerminal,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode duplicate formula output");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed formula input")
    };
    assert_eq!(input.name, "Input");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_retains_a_typed_input_with_ambiguous_formula_definitions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::DuplicateIntermediate,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode ambiguous intermediate formula output");
    let [input, intermediate, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("scalar fallback and downstream formula parameters")
    };

    assert_eq!(input.name, "Input");
    assert_eq!(intermediate.name, "Intermediate");
    assert_eq!(intermediate.expression, "2 mm");
    assert_eq!(
        intermediate.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
    assert!(intermediate.dependencies.is_empty());
    assert_eq!(output.expression, "#2_ /3+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&intermediate.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_rejects_an_incompatible_downstream_formula_without_erasing_its_input() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::IncompatibleDownstream,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode incompatible downstream formula");
    let [input, intermediate] = decoded.ir().model.parameters.as_slice() else {
        panic!("upstream formula parameters")
    };

    assert_eq!(input.name, "Input");
    assert_eq!(intermediate.name, "Intermediate");
    assert_eq!(intermediate.expression, "#1_ /2+1mm");
    assert_eq!(intermediate.dependencies, std::slice::from_ref(&input.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_does_not_infer_a_fallback_from_conflicting_formula_input_types() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode conflicting formula input types");
    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the unambiguous scalar root")
    };

    assert_eq!(input.name, "Input");
    assert!(input.dependencies.is_empty());
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_rejects_a_cyclic_formula_component() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::Cyclic,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode cyclic formula component");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_rejects_a_formula_exceeding_the_expression_depth_limit() {
    let boundary_expression = format!("{}#1_ /2", "+".repeat(128));
    let boundary = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "LENGTH",
                "LENGTH",
                12.0,
                12.0,
                &boundary_expression,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula at depth limit");
    assert_eq!(boundary.ir().model.parameters.len(), 2);

    let expression = format!("{}#1_ /2", "+".repeat(129));
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "LENGTH",
                "LENGTH",
                12.0,
                12.0,
                &expression,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode depth-limited formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Thickness");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_a_formula_with_ambiguous_input_binding() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_relation(5, true)),
            &DecodeOptions::default(),
        )
        .expect("decode ambiguous formula");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::AMBIGUOUS_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        1
    );
}
