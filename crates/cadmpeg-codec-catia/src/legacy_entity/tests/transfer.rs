// SPDX-License-Identifier: Apache-2.0
//! Legacy-entity dump tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn decode_accounts_for_unresolved_legacy_entity_runs() {
    let mut bytes = zero_entity_catpart();
    for (entity_id, lead) in [(1_u32, 0x81), (3, 0xe5), (8, 0xfd)] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([lead, 0xfd, 0x8c]);
    }
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode legacy identity run");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ENTITY_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ENTITY_IDENTITY_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_81_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_82_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_E5_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_FD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_SELECTOR_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.message.contains("legacy design run")
    }));
}

#[test]
fn decode_retains_compound_legacy_text_fields_and_relation_roles() {
    fn compound_field(bytes: &mut Vec<u8>, value: &str, role: &str, selector_low: u8) {
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend(value.as_bytes());
        bytes.push(u8::try_from(role.len() + 1).expect("short role"));
        bytes.extend(role.as_bytes());
        bytes.extend([0xe3, selector_low]);
    }

    fn selected_compound_field(
        bytes: &mut Vec<u8>,
        value: &str,
        role_selector: u8,
        selector_low: u8,
    ) {
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend(value.as_bytes());
        bytes.extend([role_selector, 0xe3, selector_low]);
    }

    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0xe5);
    compound_field(&mut bytes, "", "body", 0x53);
    compound_field(&mut bytes, "2 * #1_", "param", 0x52);
    compound_field(&mut bytes, "(#1_ : #In LENGTH) : LENGTH\n", "opened", 0x51);
    bytes.push(0xea);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0xfd);
    bytes.extend([0xa2, 0xe3, 0xa0]);
    selected_compound_field(&mut bytes, "", 0xcf, 0x9f);
    selected_compound_field(&mut bytes, "#1_ + #2_", 0xd1, 0x9e);
    selected_compound_field(
        &mut bytes,
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : LENGTH\n",
        0xd3,
        0x9d,
    );
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode compound legacy fields");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_TEXT_FIELD_COUNT),
        6
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_E3_ROLE_TAIL_TEXT_FIELD_COUNT),
        6
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_TEXT_FIELD_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SELECTED_ROLE_COUNT),
        4
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SCHEMA_FIELD_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_RELATION_COUNT),
        2
    );

    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load compound legacy fields");
    assert!(native.legacy_entity_runs[0]
        .text_fields
        .iter()
        .all(|field| {
            field.encoding == crate::native::CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
        }));
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].expression,
        "2 * #1_"
    );
    assert_eq!(
        native.legacy_entity_runs[0].relations[1].expression,
        "#1_ + #2_"
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[3]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&crate::native::CatiaLegacyRoleName::Selector(0xa2), 4769))
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[4]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&crate::native::CatiaLegacyRoleName::Selector(0xcf), 4768))
    );

    let mut invalid_relation_pair = native.clone();
    let prelude = invalid_relation_pair.legacy_entity_runs[0].text_fields[3].clone();
    invalid_relation_pair.legacy_entity_runs[0].relations[1].expression_offset =
        prelude.byte_offset;
    invalid_relation_pair.legacy_entity_runs[0].relations[1].expression = prelude.value;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid_relation_pair
        .store(&mut namespace)
        .expect("store invalid selected relation pair");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].role_selectors[3].name =
        crate::native::CatiaLegacyRoleName::Selector(0);
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid selected role");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn decode_retains_legacy_relation_synchronous_states() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0x81, 0xfd, 0x8c]);
    for (selector, state) in [(15108_u32, 0x81), (15109, 0x82)] {
        bytes.extend([
            10, b's', b'y', b'n', b'c', b'h', b'r', b'o', b'n', b'e', 0x80,
        ]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend([0xe8, 0x00, 0x1c, 0x01, state, 0xfe]);
    }
    bytes.extend([0xa3, 0xe3, 0x3c, 0xe8, 0x00, 0x1c, 0x01, 0x82]);
    bytes.extend([0xa4, 0xe3, 0x3d, 0xe8, 0x34, 0x17, 0x01, 0xfe]);
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode legacy relation update states");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SYNCHRONOUS_STATE_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SYNCHRONOUS_RELATION_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ASYNCHRONOUS_RELATION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SCHEMA_FIELD_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT),
        4
    );
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load retained update states");
    assert_eq!(
        native.legacy_entity_runs[0]
            .synchronous_states
            .iter()
            .map(|state| (state.selector, state.synchronous))
            .collect::<Vec<_>>(),
        [(15108, false), (15109, true), (4669, true)]
    );
    assert_eq!(
        native.legacy_entity_runs[0]
            .schema_fields
            .iter()
            .map(|field| (field.field_code, field.payload.as_slice()))
            .collect::<Vec<_>>(),
        [
            (0x1c00, &[0x81, 0xfe][..]),
            (0x1c00, &[0x82, 0xfe][..]),
            (0x1c00, &[0x82][..]),
        ]
    );

    let mut missing_selected_successor = native.clone();
    missing_selected_successor.legacy_entity_runs[0]
        .role_selectors
        .pop();
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    missing_selected_successor
        .store(&mut namespace)
        .expect("store selected state without successor role");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_field_boundary = native.clone();
    invalid_field_boundary.legacy_entity_runs[0].schema_fields[0].boundary_role_byte_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid_field_boundary
        .store(&mut namespace)
        .expect("store invalid schema-field boundary");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut missing_bound_field_code = native.clone();
    missing_bound_field_code.legacy_entity_runs[0].role_selectors[0].field_code = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    missing_bound_field_code
        .store(&mut namespace)
        .expect("store schema field without its role binding");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].synchronous_states[0].selector += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid relation update state");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_parameter() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    bytes.extend(12.5_f64.to_bits().to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy parameter");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy parameter")
    };
    assert_eq!(parameter.name, "Width");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.5)
        ))
    );
    assert_eq!(parameter.expression, "12.5 mm");
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("catia:legacy:entity-run#")));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_string() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([
        12, b'R', b'e', b's', b'p', b'o', b'n', b's', b'i', b'b', b'l', b'e', 0xfe,
    ]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'S', b't', b'r', b'i', b'n', b'g', 0x83]);
    bytes.extend(b"\xfe\x85\x93\x82\xfe");
    bytes.extend([
        12, b'C', b'i', b'l', b'a', b's', b' ', b'E', b'v', b'a', b'n', b's',
    ]);
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy string");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy string")
    };
    assert_eq!(parameter.name, "Responsible");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "Cilas Evans".to_string()
        ))
    );
    assert_eq!(parameter.expression, "\"Cilas Evans\"");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_STRING_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_NAMED_STRING_VALUE_COUNT),
        1
    );
}

#[test]
fn decode_transfers_an_input_bound_legacy_string_formula() {
    fn named_string(bytes: &mut Vec<u8>, entity_id: u32, name: &str, value: &str) {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1]);
        bytes.push(u8::try_from(entity_id - 1).expect("small name selector"));
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(name.len() + 1).expect("short parameter name"));
        bytes.extend(name.as_bytes());
        bytes.push(0xfe);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.extend([7, b'S', b't', b'r', b'i', b'n', b'g', 0x83]);
        bytes.extend(b"\xfe\x85\x93\x82\xfe");
        bytes.push(u8::try_from(value.len() + 1).expect("short string value"));
        bytes.extend(value.as_bytes());
    }

    fn relation_field(bytes: &mut Vec<u8>, role: &str, selector: &[u8], value: &str) {
        bytes.push(u8::try_from(role.len() + 1).expect("short relation role"));
        bytes.extend(role.as_bytes());
        bytes.extend(selector);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short relation text"));
        bytes.extend(value.as_bytes());
        bytes.push(0xfe);
    }

    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0x81, 0xfd, 0x8c]);
    relation_field(
        &mut bytes,
        "body",
        &[0x80, 1, 0, 0, 0],
        "#3_ = #1_ + \"-\" + #2_",
    );
    relation_field(
        &mut bytes,
        "param",
        &[0xd1, 3],
        "(#1_ : #In String,#2_ : #In String,#3_ : #Out String) : VoidType\n",
    );
    named_string(&mut bytes, 2, "#1_", "left");
    named_string(&mut bytes, 3, "#2_", "right");
    named_string(&mut bytes, 4, "Result", "left-right");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode input-bound legacy string formula");
    let result = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .expect("legacy formula result parameter");
    assert_eq!(
        result.value,
        Some(cadmpeg_ir::ParameterValue::String("left-right".to_string()))
    );
    assert_eq!(result.expression, "#1_ + \"-\" + #2_");
    let dependency_names = result
        .dependencies
        .iter()
        .map(|dependency| {
            decoded
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *dependency)
                .expect("legacy formula dependency")
                .name
                .as_str()
        })
        .collect::<Vec<_>>();
    assert_eq!(dependency_names, ["#1_", "#2_"]);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_integer() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'C', b'o', b'u', b'n', b't', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([8, b'I', b'n', b't', b'e', b'g', b'e', b'r', 0x83]);
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy integer");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy integer")
    };
    assert_eq!(parameter.name, "Count");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(11))
    );
    assert_eq!(parameter.expression, "11");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_INTEGER_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_NAMED_INTEGER_VALUE_COUNT),
        1
    );
}

#[test]
fn decode_transfers_an_unset_typed_legacy_parameter() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe7");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode unset typed legacy parameter");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy parameter")
    };
    assert_eq!(parameter.name, "Width");
    assert_eq!(parameter.value, None);
    assert!(parameter.expression.is_empty());
    assert_eq!(parameter.properties["value_type"], "LENGTH");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_unset_non_numeric_legacy_parameters() {
    for parameter_type in ["Boolean", "String"] {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([6, b'V', b'a', b'l', b'u', b'e', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short parameter type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe\xe7");
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode unset non-numeric legacy parameter");
        let [parameter] = decoded.ir().model.parameters.as_slice() else {
            panic!("one transferred legacy parameter")
        };

        assert_eq!(parameter.name, "Value");
        assert_eq!(parameter.value, None);
        assert!(parameter.expression.is_empty());
        assert_eq!(parameter.properties["value_type"], parameter_type);
        assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn decode_transfers_intrinsically_typed_evaluated_string_and_integer_parameters() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([0x58, 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01\x09Revision\xfe");
    bytes.extend([0x5f, 0xd1, 9]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x93\x82\xfe\x0bRevision-1");
    bytes.push(0xea);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([0x58, 0xd1, 10]);
    bytes.extend(b"\xe8\x00\x12\x01\x07Search\xfe");
    bytes.extend([6, b'V', b'a', b'l', b'b', b'y', 0xd1, 11]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x80");
    bytes.extend((-7_i32).to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode intrinsically typed evaluated parameters");

    let [string, integer] = decoded.ir().model.parameters.as_slice() else {
        panic!("two transferred evaluated parameters")
    };
    assert_eq!(string.name, "Revision");
    assert_eq!(
        string.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "Revision-1".to_string()
        ))
    );
    assert_eq!(string.expression, "\"Revision-1\"");
    assert_eq!(string.properties["value_type"], "String");
    assert_eq!(integer.name, "Search");
    assert_eq!(
        integer.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(-7))
    );
    assert_eq!(integer.properties["value_type"], "Integer");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        2
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_does_not_override_a_string_value_type_descriptor() {
    for descriptor in [
        b"\xfe\x84\x92\x82\x08Integer\x83".as_slice(),
        b"\xfe\x84\x92\x82\x82\x83".as_slice(),
    ] {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([0x58, 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01\x06Value\xfe");
        bytes.extend([0x5f, 0xd1, 9]);
        bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
        bytes.extend(descriptor);
        bytes.extend(b"\xfe\x85\x93\x82\xfe\x05Text");
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode string with an incompatible or unresolved descriptor");

        assert!(decoded.ir().model.parameters.is_empty());
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
            0
        );
    }
}

#[test]
fn decode_rejects_a_legacy_parameter_with_multiple_type_descriptors() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    for value_type in [b"LENGTH".as_slice(), b"Real".as_slice()] {
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(value_type.len() + 1).expect("short type"));
        bytes.extend(value_type);
        bytes.push(0x83);
    }
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    bytes.extend(12.5_f64.to_bits().to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode ambiguous legacy parameter");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        0
    );
}

#[test]
fn decode_resolves_only_an_acyclic_unique_legacy_type_selector_chain() {
    fn selected_type(terminal: Option<&str>) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82\x84\x83");
        bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
        bytes.extend(8.0_f64.to_bits().to_le_bytes());
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend(b"\xfe\x84\x92\x82");
        if let Some(value_type) = terminal {
            bytes.push(u8::try_from(value_type.len() + 1).expect("short type"));
            bytes.extend(value_type.as_bytes());
            bytes.push(0x83);
        } else {
            bytes.extend([0x81, 0x83]);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(selected_type(Some("LENGTH"))),
            &DecodeOptions::default(),
        )
        .expect("decode selected legacy type");
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(8.0)
        ))
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT),
        1
    );

    let cyclic = CatiaCodec
        .decode(
            &mut Cursor::new(selected_type(None)),
            &DecodeOptions::default(),
        )
        .expect("decode cyclic legacy type");
    assert!(cyclic.ir().model.parameters.is_empty());
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT),
        0
    );
}

#[test]
fn decode_transfers_only_an_agreeing_closed_legacy_formula() {
    fn legacy_constant(
        expression: &str,
        stored: Option<f64>,
        parameter_type: &str,
        relation_type: &str,
    ) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        let signature = format!("() : {relation_type}\n");
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, signature.as_str()),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([7, b'R', b'e', b's', b'u', b'l', b't', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe");
        if let Some(stored) = stored {
            bytes.push(0xe6);
            bytes.extend(stored.to_bits().to_le_bytes());
        } else {
            bytes.push(0xe7);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", Some(5.0), "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy formula");
    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one legacy formula parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);

    let mismatched = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", Some(6.0), "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched legacy formula");
    assert_eq!(mismatched.ir().model.parameters[0].expression, "6");
    assert_eq!(
        mismatched
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", None, "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode unset closed legacy formula");
    let [parameter] = unset.ir().model.parameters.as_slice() else {
        panic!("one unset legacy formula parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(parameter.value, None);
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );

    let mismatched_unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", None, "LENGTH", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode type-mismatched unset legacy formula");
    let [parameter] = mismatched_unset.ir().model.parameters.as_slice() else {
        panic!("one unset legacy parameter")
    };
    assert!(parameter.expression.is_empty());
    assert_eq!(parameter.value, None);
    assert_eq!(
        mismatched_unset
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let boolean = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("not false", None, "Boolean", "Boolean")),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean negation formula");
    let [parameter] = boolean.ir().model.parameters.as_slice() else {
        panic!("one Boolean formula parameter")
    };
    assert_eq!(parameter.expression, "not false");
    assert_eq!(parameter.value, None);
    assert_eq!(
        parameter.properties.get("value_type").map(String::as_str),
        Some("Boolean")
    );
    assert_eq!(
        boolean
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );

    let conditional = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant(
                "true ? 5 ; 1 / 0",
                Some(5.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode lazy conditional formula");
    let [parameter] = conditional.ir().model.parameters.as_slice() else {
        panic!("one conditional formula parameter")
    };
    assert_eq!(parameter.expression, "true ? 5 ; 1 / 0");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(
        conditional
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
}

#[test]
fn decode_transfers_a_zero_input_legacy_output_assignment() {
    fn legacy_output_assignment(
        expression: &str,
        stored: Option<f64>,
        parameter_type: &str,
        output_type: &str,
    ) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        let signature = format!("(#1_ : #Out {output_type}) : VoidType\n");
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, signature.as_str()),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([7, b'R', b'e', b's', b'u', b'l', b't', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe");
        if let Some(stored) = stored {
            bytes.push(0xe6);
            bytes.extend(stored.to_bits().to_le_bytes());
        } else {
            bytes.push(0xe7);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let transferred = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment(
                "#1_ = 2+3",
                Some(5.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode output assignment");
    let [parameter] = transferred.ir().model.parameters.as_slice() else {
        panic!("one legacy output parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(
        transferred
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment("#1_ = 2+3", None, "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode unset output assignment");
    assert_eq!(unset.ir().model.parameters[0].expression, "2+3");
    assert_eq!(unset.ir().model.parameters[0].value, None);

    let mismatched_value = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment(
                "#1_ = 2+3",
                Some(6.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched output assignment");
    assert_eq!(mismatched_value.ir().model.parameters[0].expression, "6");
    assert_eq!(
        mismatched_value
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let mismatched_type = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment(
                "#1_ = 2+3",
                None,
                "LENGTH",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode type-mismatched output assignment");
    assert!(mismatched_type.ir().model.parameters[0]
        .expression
        .is_empty());
    assert_eq!(
        mismatched_type
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );
}

#[test]
fn decode_transfers_an_agreeing_closed_legacy_string_formula() {
    fn legacy_string_constant(expression: &str, stored: &str) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, "() : String\n"),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([0x58, 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01\x0fNewResponsible\xfe");
        bytes.extend([0x5f, 0xd1, 9]);
        bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
        bytes.extend(b"\xfe\x85\x93\x82\xfe");
        bytes.push(u8::try_from(stored.len() + 1).expect("short stored string"));
        bytes.extend(stored.as_bytes());
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")",
                "Easy Evans",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy string formula");
    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one legacy string formula parameter")
    };
    assert_eq!(
        parameter.expression,
        "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")"
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::String("Easy Evans".to_string()))
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());

    let mismatched = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")",
                "Cilas Evans",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched legacy string formula");
    assert_eq!(
        mismatched.ir().model.parameters[0].expression,
        "\"Cilas Evans\""
    );
    assert_eq!(
        mismatched
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let methods = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ToLower(\"MIXED\").Extract(1,4) - \"x\"",
                "ied",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy string-method formula");
    let [parameter] = methods.ir().model.parameters.as_slice() else {
        panic!("one legacy string-method formula parameter")
    };
    assert_eq!(
        parameter.expression,
        "ToLower(\"MIXED\").Extract(1,4) - \"x\""
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::String("ied".to_string()))
    );
    assert_eq!(
        methods
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
}
