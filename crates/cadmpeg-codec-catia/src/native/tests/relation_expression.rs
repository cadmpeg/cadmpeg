// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_namespace_types_and_validates_complete_relation_expressions() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression("param"));
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("complete relation expression");
    let crate::native::CatiaRelationExpressionFraming::PlaceholderState {
        placeholder,
        state_role,
    } = &expression.framing
    else {
        panic!("placeholder-state framing")
    };
    assert_eq!(placeholder.value, "#1_ ");
    assert_eq!(state_role.value, "opened");
    assert_eq!(expression.expression.value, "#1_ /2-2mm");
    assert_eq!(expression.parameter_role.value, "param");
    assert_eq!(
        expression.type_signature.value,
        "(#1_ : #In LENGTH) : LENGTH"
    );
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature.inputs,
        [crate::native::CatiaRelationTypeInput {
            parameter: "#1_".to_string(),
            input_type: "LENGTH".to_string(),
        }]
    );
    assert_eq!(signature.result_type, "LENGTH");
    assert_eq!(expression.function_role.value, "RelationExpFct");

    let mut malformed = native;
    malformed.entity_records[0]
        .relation_expression
        .as_mut()
        .expect("complete relation expression")
        .expression
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed relation expression");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_parser_version_relation_expression("Boolean", "ParserVersion"),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::BooleanParserVersion {
        prefix_role,
        parser_version_role,
    } = &expression.framing
    else {
        panic!("parser-version framing")
    };
    assert_eq!(prefix_role.value, "Boolean");
    assert_eq!(
        expression.expression.value,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2"
    );
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(expression.parameter_role.value, "param");
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature
            .inputs
            .iter()
            .map(|input| (input.parameter.as_str(), input.input_type.as_str()))
            .collect::<Vec<_>>(),
        [("#1_", "LENGTH"), ("#2_", "LENGTH")]
    );
    assert_eq!(signature.result_type, "Real");
}

#[test]
fn opened_parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_opened_parser_version_relation_expression(
            "Boolean",
            "ParserVersion",
            "opened",
        ),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("opened parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
        prefix_role,
        parser_version_role,
        state_role,
    } = &expression.framing
    else {
        panic!("opened parser-version framing")
    };
    assert_eq!(prefix_role.value, "Boolean");
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(state_role.value, "opened");
    assert_eq!(
        expression.expression.value,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2"
    );
    assert_eq!(expression.parameter_role.value, "param");
    assert!(expression.signature.is_some());
}

#[test]
fn opened_parser_version_relation_expression_requires_every_exact_role() {
    for (prefix_role, parser_version_role, state_role) in [
        ("Real", "ParserVersion", "opened"),
        ("Boolean", "ParserRevision", "opened"),
        ("Boolean", "ParserVersion", "closed"),
    ] {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_opened_parser_version_relation_expression(
                prefix_role,
                parser_version_role,
                state_role,
            ),
        );

        assert!(native.entity_records[0].relation_expression.is_none());
    }
}

#[test]
fn decode_retains_an_opened_parser_version_expression_without_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_opened_parser_version_relation_expression(
                    "Boolean",
                    "ParserVersion",
                    "opened",
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode opened parser-version expression");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_OPENED_BOOLEAN_PARSER_VERSION_RELATION_EXPRESSION_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_TYPED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
}

#[test]
fn unprefixed_parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_unprefixed_parser_version_relation_expression("ParserVersion"),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("unprefixed parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::ParserVersion {
        parser_version_role,
    } = &expression.framing
    else {
        panic!("unprefixed parser-version framing")
    };
    assert_eq!(expression.expression.value, "360.0*1 deg/#1_");
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(expression.parameter_role.value, "param");
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature.inputs,
        [crate::native::CatiaRelationTypeInput {
            parameter: "#1_".to_string(),
            input_type: "Integer".to_string(),
        }]
    );
    assert_eq!(signature.result_type, "ANGLE");
}

#[test]
fn unprefixed_parser_version_relation_expression_requires_the_exact_version_role() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_unprefixed_parser_version_relation_expression("ParserRevision"),
    );

    assert!(native.entity_records[0].relation_expression.is_none());
}

#[test]
fn decode_retains_an_unprefixed_parser_version_expression_without_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_unprefixed_parser_version_relation_expression(
                    "ParserVersion",
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unprefixed parser-version expression");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_PARSER_VERSION_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_TYPED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
}

#[test]
fn parser_version_relation_expression_requires_both_exact_framing_roles() {
    for (prefix_role, parser_version_role) in
        [("Real", "ParserVersion"), ("Boolean", "ParserRevision")]
    {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_parser_version_relation_expression(
                prefix_role,
                parser_version_role,
            ),
        );

        assert!(native.entity_records[0].relation_expression.is_none());
    }
}

#[test]
fn decode_retains_a_parser_version_expression_without_fabricating_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parser_version_relation_expression(
                "Boolean",
                "ParserVersion",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode parser-version expression");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
}

#[test]
fn relation_expression_signature_preserves_ordered_typed_inputs() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ :  #In LENGTH,#2_ :  #In ANGLE) : Real",
        ));
    let signature = native.entity_records[0]
        .relation_expression
        .as_ref()
        .and_then(|expression| expression.signature.as_ref())
        .expect("multi-input signature");

    assert_eq!(
        signature.inputs,
        [
            crate::native::CatiaRelationTypeInput {
                parameter: "#1_".to_string(),
                input_type: "LENGTH".to_string(),
            },
            crate::native::CatiaRelationTypeInput {
                parameter: "#2_".to_string(),
                input_type: "ANGLE".to_string(),
            },
        ]
    );
    assert_eq!(signature.result_type, "Real");
}

#[test]
fn relation_expression_signature_accepts_an_empty_input_list_with_an_empty_placeholder() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "", "() : LENGTH"),
    );
    let signature = native.entity_records[0]
        .relation_expression
        .as_ref()
        .and_then(|expression| expression.signature.as_ref())
        .expect("zero-input signature");

    assert!(signature.inputs.is_empty());
    assert_eq!(signature.result_type, "LENGTH");

    let nonempty_placeholder = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "#1_ ", "() : LENGTH"),
    );
    assert!(nonempty_placeholder.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_requires_exact_outer_whitespace() {
    for signature in [
        "( ) : LENGTH",
        "() :  LENGTH",
        "() : LENGTH ",
        "() : LENGTH\n\n",
    ] {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_relation_expression_signature("param", "", signature),
        );

        assert!(
            native.entity_records[0]
                .relation_expression
                .as_ref()
                .expect("relation expression")
                .signature
                .is_none(),
            "{signature:?}"
        );
    }
}

#[test]
fn native_migrates_and_validates_relation_signature_outer_whitespace() {
    let mut native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "", "( ) : LENGTH"),
    );
    let entity = &mut native.entity_records[0];
    let expression = entity
        .relation_expression
        .as_mut()
        .expect("relation expression");
    assert!(expression.signature.is_none());
    expression.signature = Some(crate::native::CatiaRelationTypeSignature {
        inputs: Vec::new(),
        result_type: "LENGTH".to_string(),
    });

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store pre-canonical signature");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION - 1)
            .unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate signature whitespace");
    assert!(migrated.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_rejects_duplicate_inputs() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ : #In LENGTH,#1_ : #In ANGLE) : Real",
        ));

    assert!(native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_requires_canonical_parameter_symbols() {
    for parameter in ["value", "#_", "#1", "#1_ /2", "#１_"] {
        let signature = format!("({parameter} : #In LENGTH) : Real");
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_relation_expression_signature("param", parameter, &signature),
        );

        assert!(
            native.entity_records[0]
                .relation_expression
                .as_ref()
                .expect("relation expression")
                .signature
                .is_none(),
            "{parameter}"
        );
    }
}

#[test]
fn native_migrates_and_validates_relation_signature_parameter_symbols() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ : #In LENGTH) : Real",
        ));
    let expected = native.entity_records[0]
        .relation_expression
        .clone()
        .expect("relation expression");
    let mut malformed = native;
    malformed.entity_records[0]
        .relation_expression
        .as_mut()
        .expect("relation expression")
        .signature
        .as_mut()
        .expect("typed signature")
        .inputs[0]
        .parameter = "value".to_string();

    let mut current_namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut current_namespace)
        .expect("store malformed relation signature");
    assert!(matches!(
        crate::native::CatiaNative::load(&current_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    current_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_RELATION_SIGNATURE_PARAMETER_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&current_namespace)
        .expect("migrate relation signature parameters");
    assert_eq!(
        migrated.entity_records[0].relation_expression,
        Some(expected)
    );
}

#[test]
fn relation_expression_requires_every_exact_role() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression("parameter"));

    assert!(native.entity_records[0].relation_expression.is_none());
}

#[test]
fn relation_expression_signature_requires_the_selected_placeholder() {
    let mut file = standard_catpart_with_relation_expression("param");
    let signature = file
        .windows("(#1_ : #In LENGTH) : LENGTH".len())
        .position(|bytes| bytes == b"(#1_ : #In LENGTH) : LENGTH")
        .expect("relation type signature");
    file[signature + 2] = b'2';

    let native = crate::native::CatiaNative::decode(&file);
    assert!(native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("complete relation expression")
        .signature
        .is_none());
}
