// SPDX-License-Identifier: Apache-2.0
//! Legacy-entity tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

mod identity;
mod transfer;

use super::*;
use crate::container;

fn identity(bytes: &mut Vec<u8>, entity_id: u32) {
    identity_with_lead(bytes, entity_id, 0x81);
}

fn identity_with_lead(bytes: &mut Vec<u8>, entity_id: u32, lead: u8) {
    bytes.push(0xea);
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.push(lead);
    bytes.extend_from_slice(&[0xfd, 0x8c]);
}

#[test]
fn role_selector_boundary_rejects_offset_overflow() {
    let role = LegacyRoleSelector {
        offset: usize::MAX,
        entity_id: 1,
        name: LegacyRoleName::Literal("body".to_string()),
        encoding: LegacyRoleSelectorEncoding::Paged,
        selector: 1,
        field_code: None,
    };
    assert_eq!(role.end_offset(), None);
}

#[test]
fn retains_a_schema_program_closed_by_the_first_complete_footer() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(CATALOG_OPEN);
    bytes.extend_from_slice(SCHEMA_PROGRAM_PREFIX);
    let program_offset = bytes.len();
    bytes.extend_from_slice(&[0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    let footer_offset = bytes.len();
    bytes.extend_from_slice(SCHEMA_PROGRAM_FOOTER);

    let runs = parse_runs(&bytes);
    let program = runs[0]
        .schema_program
        .as_ref()
        .expect("complete schema program");
    assert_eq!(program.offset, program_offset);
    assert_eq!(program.boundary_offset, footer_offset);
    assert_eq!(
        program.boundary,
        super::LegacySchemaProgramBoundary::VendorFooter
    );
    assert_eq!(program.bytes, [0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    assert_eq!(
        program.identifiers,
        [super::LegacySchemaIdentifier {
            offset: program_offset + 1,
            value: "Foo".to_string(),
        }]
    );

    let mut unterminated = bytes.clone();
    unterminated[footer_offset - 1] = 0x81;
    assert!(parse_runs(&unterminated)[0].schema_program.is_none());

    let mut repeated_footer = bytes.clone();
    repeated_footer.splice(
        footer_offset..footer_offset,
        SCHEMA_PROGRAM_FOOTER.iter().copied(),
    );
    let repeated_runs = parse_runs(&repeated_footer);
    let repeated = repeated_runs[0]
        .schema_program
        .as_ref()
        .expect("first complete footer closes the program");
    assert_eq!(repeated.boundary_offset, footer_offset);

    let mut incomplete_footer = bytes;
    let incomplete_footer_offset = program_offset + 1;
    incomplete_footer.splice(
        incomplete_footer_offset..incomplete_footer_offset,
        SCHEMA_PROGRAM_FOOTER.iter().copied(),
    );
    let incomplete_runs = parse_runs(&incomplete_footer);
    let program = incomplete_runs[0]
        .schema_program
        .as_ref()
        .expect("incomplete footer does not shadow a complete footer");
    assert_eq!(
        program.boundary_offset,
        footer_offset + SCHEMA_PROGRAM_FOOTER.len()
    );
}

#[test]
fn retains_a_schema_program_closed_by_the_outer_directory() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(CATALOG_OPEN);
    bytes.extend_from_slice(SCHEMA_PROGRAM_PREFIX);
    let program_offset = bytes.len();
    bytes.extend_from_slice(&[0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    let directory_offset = bytes.len();
    bytes.extend_from_slice(container::DIR_MAGIC);

    let runs = parse_runs_with_directory_offset(&bytes, Some(directory_offset));
    let program = runs[0]
        .schema_program
        .as_ref()
        .expect("directory-bound schema program");
    assert_eq!(program.offset, program_offset);
    assert_eq!(program.boundary_offset, directory_offset);
    assert_eq!(
        program.boundary,
        super::LegacySchemaProgramBoundary::StreamDirectory
    );
    assert_eq!(program.bytes, [0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);

    let mut unterminated = bytes;
    unterminated[directory_offset - 1] = 0x81;
    assert!(
        parse_runs_with_directory_offset(&unterminated, Some(directory_offset))[0]
            .schema_program
            .is_none()
    );
}

#[test]
fn selected_roles_bind_following_schema_fields() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(&[0xa1, 0xe3, 0x5b, 0xe8, 0x28, 0x17, 0x01, 0xfe]);
    bytes.extend_from_slice(&[0xa2, 0xe3, 0x3b, 0xe8, 0x00, 0x1c, 0x01, 0x82]);
    bytes.extend_from_slice(&[
        0xa4, 0x80, 0xd5, 0xc4, 0x01, 0x00, 0xe8, 0x34, 0x17, 0x01, 0xfe,
    ]);
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    assert_eq!(
        run.role_selectors
            .iter()
            .map(|role| (&role.name, role.encoding, role.selector, role.field_code))
            .collect::<Vec<_>>(),
        [
            (
                &LegacyRoleName::Selector(0xa1),
                LegacyRoleSelectorEncoding::Paged,
                4700,
                Some(0x1728),
            ),
            (
                &LegacyRoleName::Selector(0xa2),
                LegacyRoleSelectorEncoding::Paged,
                4668,
                Some(0x1c00),
            ),
            (
                &LegacyRoleName::Selector(0xa4),
                LegacyRoleSelectorEncoding::FixedU32,
                115_925,
                Some(0x1734),
            ),
        ]
    );
    assert_eq!(run.synchronous_states.len(), 1);
    assert_eq!(run.synchronous_states[0].selector, 4668);
    assert!(run.synchronous_states[0].synchronous);
    assert_eq!(
        run.schema_fields
            .iter()
            .map(|field| (
                field.field_code,
                field.payload.as_slice(),
                field.role_offset,
                field.boundary_role_offset,
            ))
            .collect::<Vec<_>>(),
        [(0x1728, &[0xfe][..], 8, 16), (0x1c00, &[0x82][..], 16, 24),]
    );
}

#[test]
fn parses_monotone_identity_suffix_before_legacy_catalog() {
    let mut bytes = vec![0xea, 9, 0, 0, 0, 0x81];
    identity(&mut bytes, 1);
    identity(&mut bytes, 4);
    identity(&mut bytes, 7);
    let catalog_offset = bytes.len();
    bytes.extend_from_slice(CATALOG_OPEN);

    let runs = parse_runs(&bytes);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].catalog_offset, catalog_offset);
    assert!(runs[0].text_fields.is_empty());
    assert!(runs[0].relations.is_empty());
    assert!(runs[0].synchronous_states.is_empty());
    assert!(runs[0].type_descriptors.is_empty());
    assert!(runs[0].scalar_values.is_empty());
    assert!(runs[0].string_values.is_empty());
    assert!(runs[0].integer_values.is_empty());
    assert_eq!(
        runs[0]
            .identities
            .iter()
            .map(|identity| identity.entity_id)
            .collect::<Vec<_>>(),
        [1, 4, 7]
    );
    assert!(runs[0]
        .identities
        .iter()
        .all(|identity| identity.lead == 0x81));
}

#[test]
fn parses_each_admitted_identity_record_lead() {
    let mut bytes = Vec::new();
    for (entity_id, lead) in [(1, 0x81), (2, 0x82), (3, 0xe5), (4, 0xfd)] {
        identity_with_lead(&mut bytes, entity_id, lead);
    }
    bytes.extend_from_slice(CATALOG_OPEN);

    assert_eq!(
        parse_runs(&bytes)[0]
            .identities
            .iter()
            .map(|identity| (identity.entity_id, identity.lead))
            .collect::<Vec<_>>(),
        [(1, 0x81), (2, 0x82), (3, 0xe5), (4, 0xfd)]
    );
}

#[test]
fn unsupported_record_leads_do_not_split_identity_intervals() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    identity_with_lead(&mut bytes, 2, 0xe6);
    identity(&mut bytes, 3);
    bytes.extend_from_slice(CATALOG_OPEN);

    assert_eq!(
        parse_runs(&bytes)[0]
            .identities
            .iter()
            .map(|identity| identity.entity_id)
            .collect::<Vec<_>>(),
        [1, 3]
    );
}

#[test]
fn rejects_suffix_that_does_not_begin_with_identity_one() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    identity(&mut bytes, 4);
    identity(&mut bytes, 2);
    bytes.extend_from_slice(CATALOG_OPEN);

    assert!(parse_runs(&bytes).is_empty());
}

#[test]
fn parses_each_closed_schema_text_production() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0xfe]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.push(0);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.extend_from_slice(b"line\n");
    bytes.push(0xfe);
    bytes.extend_from_slice(CATALOG_OPEN);

    let fields = &parse_runs(&bytes)[0].text_fields;
    assert_eq!(
        fields
            .iter()
            .map(|field| (field.encoding, field.value.as_str()))
            .collect::<Vec<_>>(),
        [
            (super::LegacyTextEncoding::U8InclusiveLength, "name"),
            (super::LegacyTextEncoding::ZeroU32Length, "line\n"),
        ]
    );
    assert!(fields.iter().all(|field| field.entity_id == 1));
    assert!(fields.iter().all(|field| field.role.is_none()));
}

#[test]
fn binds_immediately_preceding_role_selectors_to_text_fields() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(&[5, b'b', b'o', b'd', b'y', 0xe1, 0x25]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[5, b'r', b'u', b'l', b'e', 0xfe]);
    bytes.extend_from_slice(&[6, b'p', b'a', b'r', b'a', b'm', 0x80]);
    bytes.extend_from_slice(&15108_u32.to_le_bytes());
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[5, b't', b'y', b'p', b'e', 0xfe]);
    bytes.extend_from_slice(&[8, b'p', b'a', b'r', b'a', b'm', b'i', b'n', 0xd1, 0x2a]);
    bytes.extend_from_slice(&[0xe8, 0xe4, 0x0b, 0x01]);
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    let fields = &run.text_fields;
    let body = fields[0].role.as_ref().expect("paged role selector");
    assert_eq!(body.name.literal(), Some("body"));
    assert_eq!(body.selector, 4134);
    assert_eq!(body.encoding, super::LegacyRoleSelectorEncoding::Paged);
    let parameter = fields[1].role.as_ref().expect("fixed role selector");
    assert_eq!(parameter.name.literal(), Some("param"));
    assert_eq!(parameter.selector, 15108);
    assert_eq!(
        parameter.encoding,
        super::LegacyRoleSelectorEncoding::FixedU32
    );
    assert_eq!(
        run.role_selectors
            .iter()
            .filter_map(|role| role.name.literal().map(|name| (name, role.selector)))
            .collect::<Vec<_>>(),
        [("body", 4134), ("param", 15108), ("paramin", 43)]
    );
    assert_eq!(run.role_selectors[2].entity_id, 1);
}

#[test]
fn parses_complete_relation_synchronous_states() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    for (selector, state) in [(15108_u32, 0x81), (15109, 0x82)] {
        bytes.extend_from_slice(&[
            10, b's', b'y', b'n', b'c', b'h', b'r', b'o', b'n', b'e', 0x80,
        ]);
        bytes.extend_from_slice(&selector.to_le_bytes());
        bytes.extend_from_slice(&[0xe8, 0x00, 0x1c, 0x01, state, 0xfe]);
    }
    bytes.extend_from_slice(CATALOG_OPEN);

    let states = &parse_runs(&bytes)[0].synchronous_states;
    assert_eq!(
        states
            .iter()
            .map(|state| (state.selector, state.synchronous))
            .collect::<Vec<_>>(),
        [(15108, false), (15109, true)]
    );
    assert!(states.iter().all(|state| state.entity_id == 1));
}

#[test]
fn rejects_malformed_relation_synchronous_states() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    for payload in [
        [0xe8, 0x00, 0x1c, 0x01, 0x80, 0xfe],
        [0xe8, 0x00, 0x1c, 0x01, 0x81, 0xff],
        [0xe8, 0x00, 0x1d, 0x01, 0x82, 0xfe],
    ] {
        bytes.extend_from_slice(&[
            10, b's', b'y', b'n', b'c', b'h', b'r', b'o', b'n', b'e', 0x80,
        ]);
        bytes.extend_from_slice(&15108_u32.to_le_bytes());
        bytes.extend_from_slice(&payload);
    }
    bytes.extend_from_slice(CATALOG_OPEN);

    assert!(parse_runs(&bytes)[0].synchronous_states.is_empty());

    let mut crossing = Vec::new();
    identity(&mut crossing, 1);
    crossing.extend_from_slice(&[
        10, b's', b'y', b'n', b'c', b'h', b'r', b'o', b'n', b'e', 0x80,
    ]);
    crossing.extend_from_slice(&15108_u32.to_le_bytes());
    crossing.extend_from_slice(&[0xe8, 0x00, 0x1c]);
    identity(&mut crossing, 2);
    crossing.extend_from_slice(&[0x01, 0x82, 0xfe]);
    crossing.extend_from_slice(CATALOG_OPEN);

    assert!(parse_runs(&crossing)[0].synchronous_states.is_empty());
}

#[test]
fn rejects_unclosed_and_control_bearing_schema_text_candidates() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[4, b'a', 1, b'b', 0xfe]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[1, 5, b'b', b'o', b'd', b'y', 0xe3]);
    bytes.extend_from_slice(CATALOG_OPEN);

    assert!(parse_runs(&bytes)[0].text_fields.is_empty());
}

#[test]
fn pairs_expression_and_typed_signature_roles() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    for value in [
        "#2_ = #1_ + 2",
        "(#2_ : #Out Real,#1_ : #In Real) : VoidType\n",
    ] {
        bytes.extend_from_slice(TEXT_OPEN);
        bytes.push(u8::try_from(value.len() + 1).expect("short text"));
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0xfe);
    }
    bytes.extend_from_slice(CATALOG_OPEN);

    let relation = &parse_runs(&bytes)[0].relations[0];
    assert_eq!(relation.expression, "#2_ = #1_ + 2");
    assert_eq!(relation.body_selector, None);
    assert_eq!(relation.parameter_selector, None);
    assert_eq!(relation.parameter_entity_id, None);
    assert_eq!(
        relation
            .signature
            .output()
            .expect("VoidType signature has an output")
            .parameter,
        "#2_"
    );
    assert_eq!(relation.signature.inputs[0].parameter, "#1_");
    assert_eq!(relation.signature.result_type(), "VoidType");
}

#[test]
fn pairs_compound_text_fields_through_inline_role_tails() {
    fn compound_field(bytes: &mut Vec<u8>, value: &str, role: &str, selector: u8) {
        bytes.extend_from_slice(TEXT_OPEN);
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(u8::try_from(role.len() + 1).expect("short role"));
        bytes.extend_from_slice(role.as_bytes());
        bytes.extend_from_slice(&[0xe3, selector]);
    }

    fn selected_compound_field(
        bytes: &mut Vec<u8>,
        value: &str,
        role_selector: u8,
        selector_low: u8,
    ) {
        bytes.extend_from_slice(TEXT_OPEN);
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend_from_slice(value.as_bytes());
        bytes.extend_from_slice(&[role_selector, 0xe3, selector_low]);
    }

    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    identity_with_lead(&mut bytes, 93, 0xe5);
    compound_field(&mut bytes, "", "body", 0x53);
    compound_field(&mut bytes, "2 * #1_", "param", 0x52);
    compound_field(&mut bytes, "(#1_ : #In LENGTH) : LENGTH\n", "opened", 0x51);
    identity_with_lead(&mut bytes, 99, 0xfd);
    bytes.extend_from_slice(&[0xa2, 0xe3, 0xa0]);
    selected_compound_field(&mut bytes, "", 0xcf, 0x9f);
    selected_compound_field(&mut bytes, "#1_ + #2_", 0xd1, 0x9e);
    selected_compound_field(
        &mut bytes,
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : LENGTH\n",
        0xd3,
        0x9d,
    );
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    assert_eq!(run.text_fields.len(), 6);
    assert!(run
        .text_fields
        .iter()
        .all(|field| { field.encoding == super::LegacyTextEncoding::U8InclusiveLengthE3RoleTail }));
    assert_eq!(
        run.text_fields
            .iter()
            .take(3)
            .map(|field| (
                field.value.as_str(),
                field.role.as_ref().and_then(|role| role.name.literal())
            ))
            .collect::<Vec<_>>(),
        [
            ("", None),
            ("2 * #1_", Some("body")),
            ("(#1_ : #In LENGTH) : LENGTH\n", Some("param"))
        ]
    );
    assert_eq!(
        run.role_selectors
            .iter()
            .filter(|role| { matches!(role.name.literal(), Some("body" | "param" | "opened")) })
            .filter_map(|role| {
                role.name
                    .literal()
                    .map(|name| (name, role.selector, role.encoding))
            })
            .collect::<Vec<_>>(),
        [
            ("body", 4692, super::LegacyRoleSelectorEncoding::Paged),
            ("param", 4691, super::LegacyRoleSelectorEncoding::Paged),
            ("opened", 4690, super::LegacyRoleSelectorEncoding::Paged)
        ]
    );
    let relation = &run.relations[0];
    assert_eq!(relation.expression, "2 * #1_");
    assert_eq!(relation.body_selector, Some(4692));
    assert_eq!(relation.parameter_selector, Some(4691));
    assert_eq!(relation.parameter_entity_id, None);
    assert_eq!(
        run.text_fields[3]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&super::LegacyRoleName::Selector(0xa2), 4769))
    );
    assert_eq!(
        run.text_fields[4]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&super::LegacyRoleName::Selector(0xcf), 4768))
    );
    assert_eq!(
        run.text_fields[5]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&super::LegacyRoleName::Selector(0xd1), 4767))
    );
    assert_eq!(run.relations.len(), 2);
    assert_eq!(run.relations[1].expression, "#1_ + #2_");
    assert_eq!(run.relations[1].body_selector, None);
    assert_eq!(run.relations[1].parameter_selector, None);
}

#[test]
fn text_terminator_precedes_an_e3_shaped_unresolved_role() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[2, b'x', 0xfe, 0xe3, 0x17]);
    identity(&mut bytes, 2);
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    assert_eq!(run.text_fields.len(), 1);
    assert_eq!(
        run.text_fields[0].encoding,
        super::LegacyTextEncoding::U8InclusiveLength
    );
    assert_eq!(run.text_fields[0].value, "x");
    assert!(!run
        .role_selectors
        .iter()
        .any(|role| { matches!(role.name, super::LegacyRoleName::Selector(0xfe)) }));
}

#[test]
fn rejects_ambiguous_unresolved_role_selector_framing() {
    let bytes = [0x81, 0x80, 0x01, 0x82, 0xd1, 0x17, 0xe8, 0x00, 0x1c, 0x01];
    assert!(super::parse_role_selectors(&bytes, 0, bytes.len(), 1).is_empty());
}

#[test]
fn binds_exact_body_and_parameter_roles_to_a_run_identity() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    for (role, selector, value) in [
        ("body", 1_u32, "#1_ + 2"),
        ("param", 4_u32, "(#1_ : #In Real) : Real\n"),
    ] {
        bytes.push(u8::try_from(role.len() + 1).expect("short role"));
        bytes.extend_from_slice(role.as_bytes());
        bytes.push(0x80);
        bytes.extend_from_slice(&selector.to_le_bytes());
        bytes.extend_from_slice(TEXT_OPEN);
        bytes.push(u8::try_from(value.len() + 1).expect("short text"));
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0xfe);
    }
    identity(&mut bytes, 4);
    bytes.extend_from_slice(CATALOG_OPEN);

    let relation = &parse_runs(&bytes)[0].relations[0];
    assert_eq!(relation.body_selector, Some(1));
    assert_eq!(relation.parameter_selector, Some(4));
    assert_eq!(relation.parameter_entity_id, Some(4));
}

#[test]
fn parses_finite_and_unset_scalar_packets() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(SCALAR_OPEN);
    bytes.push(0xe6);
    bytes.extend_from_slice(&3.5_f64.to_bits().to_le_bytes());
    bytes.extend_from_slice(SCALAR_OPEN);
    bytes.push(0xe7);
    bytes.extend_from_slice(CATALOG_OPEN);

    let values = &parse_runs(&bytes)[0].scalar_values;
    assert_eq!(
        values
            .iter()
            .map(|value| value.evaluation)
            .collect::<Vec<_>>(),
        [
            super::LegacyScalarEvaluation::Value(3.5_f64.to_bits()),
            super::LegacyScalarEvaluation::Unset,
        ]
    );
    assert!(values
        .iter()
        .all(|value| { value.encoding == super::LegacyScalarEncoding::Standalone85 }));
}

#[test]
fn binds_a_unique_co_owned_name_role_to_a_scalar() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[8, b'L', b'e', b'n', b'g', b't', b'h', b'.', 0xfe]);
    bytes.extend_from_slice(NAMED_SCALAR_OPEN);
    bytes.push(0xe6);
    bytes.extend_from_slice(&12.0_f64.to_bits().to_le_bytes());
    bytes.extend_from_slice(CATALOG_OPEN);

    let runs = parse_runs(&bytes);
    let value = &runs[0].scalar_values[0];
    assert_eq!(value.encoding, super::LegacyScalarEncoding::Named84);
    assert_eq!(value.name.as_deref(), Some("Length."));
    assert_eq!(value.name_offset, Some(runs[0].text_fields[0].offset));
}

#[test]
fn parses_and_names_inclusive_length_string_values() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[
        12, b'R', b'e', b's', b'p', b'o', b'n', b's', b'i', b'b', b'l', b'e', 0xfe,
    ]);
    bytes.extend_from_slice(STRING_OPEN);
    bytes.extend_from_slice(&[
        12, b'C', b'i', b'l', b'a', b's', b' ', b'E', b'v', b'a', b'n', b's',
    ]);
    identity(&mut bytes, 2);
    bytes.extend_from_slice(STRING_OPEN);
    bytes.push(1);
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    assert_eq!(run.string_values.len(), 2);
    assert_eq!(run.string_values[0].value, "Cilas Evans");
    assert_eq!(run.string_values[0].name.as_deref(), Some("Responsible"));
    assert_eq!(
        run.string_values[0].name_offset,
        Some(run.text_fields[0].offset)
    );
    assert_eq!(run.string_values[1].value, "");
    assert!(run.string_values[1].name.is_none());
}

#[test]
fn evaluation_fields_bind_selected_role_names_to_typed_values() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(&[0x58, 0xd1, 8]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[7, b'R', b'e', b's', b'u', b'l', b't', 0xfe]);
    bytes.extend_from_slice(&[0x5f, 0xd1, 9, 0xe8, 0xc4, 0x17, 0x01, 0xfe, 0xfe]);
    bytes.extend_from_slice(STRING_OPEN);
    bytes.extend_from_slice(&[5, b'D', b'o', b'n', b'e']);
    identity(&mut bytes, 2);
    bytes.extend_from_slice(&[0x58, 0xd1, 10]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[6, b'C', b'o', b'u', b'n', b't', 0xfe]);
    bytes.extend_from_slice(&[6, b'V', b'a', b'l', b'b', b'y', 0xd1, 11]);
    bytes.extend_from_slice(&[0xe8, 0xc4, 0x17, 0x01, 0xfe, 0xfe]);
    bytes.extend_from_slice(INTEGER_OPEN);
    bytes.push(0x8c);
    identity(&mut bytes, 3);
    bytes.extend_from_slice(&[0x58, 0xd1, 12]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[9, b'C', b'o', b'n', b's', b't', b'a', b'n', b't', 0xfe]);
    bytes.extend_from_slice(STRING_OPEN);
    bytes.extend_from_slice(&[5, b'D', b'a', b't', b'a']);
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    assert_eq!(run.string_values[0].name.as_deref(), Some("Result"));
    assert_eq!(run.integer_values[0].name.as_deref(), Some("Count"));
    assert_eq!(run.string_values[1].value, "Data");
    assert!(run.string_values[1].name.is_none());
}

#[test]
fn parses_and_names_inline_and_wide_signed_integers() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[6, b'C', b'o', b'u', b'n', b't', 0xfe]);
    bytes.extend_from_slice(INTEGER_OPEN);
    bytes.push(0x8c);
    identity(&mut bytes, 2);
    bytes.extend_from_slice(INTEGER_OPEN);
    bytes.extend_from_slice(&[0x80, 0xff, 0xff, 0xff, 0xff]);
    identity(&mut bytes, 3);
    bytes.extend_from_slice(INTEGER_OPEN);
    bytes.push(0x80);
    bytes.extend_from_slice(CATALOG_OPEN);

    let run = &parse_runs(&bytes)[0];
    assert_eq!(run.integer_values.len(), 2);
    assert_eq!(run.integer_values[0].value, 11);
    assert_eq!(run.integer_values[0].name.as_deref(), Some("Count"));
    assert_eq!(
        run.integer_values[0].encoding,
        super::LegacyIntegerEncoding::Inline
    );
    assert_eq!(run.integer_values[1].value, -1);
    assert_eq!(
        run.integer_values[1].encoding,
        super::LegacyIntegerEncoding::WideI32
    );
    assert!(run.integer_values[1].name.is_none());
}

#[test]
fn parses_literal_and_compact_legacy_type_descriptors() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(TYPE_OPEN);
    bytes.extend_from_slice(&[8, b'B', b'o', b'o', b'l', b'e', b'a', b'n', 0x83]);
    bytes.extend_from_slice(TYPE_OPEN);
    bytes.extend_from_slice(&[0x96, 0x83]);
    bytes.extend_from_slice(CATALOG_OPEN);

    let descriptors = &parse_runs(&bytes)[0].type_descriptors;
    assert_eq!(
        descriptors
            .iter()
            .map(|descriptor| descriptor.value.clone())
            .collect::<Vec<_>>(),
        [
            super::LegacyTypeValue::Name("Boolean".to_string()),
            super::LegacyTypeValue::Selector(22),
        ]
    );
}

#[test]
fn rejects_unclosed_and_nonidentifier_type_descriptors() {
    let mut bytes = Vec::new();
    identity(&mut bytes, 1);
    bytes.extend_from_slice(TYPE_OPEN);
    bytes.extend_from_slice(&[5, b'R', b'e', b'a', b'l', 0xfe]);
    bytes.extend_from_slice(TYPE_OPEN);
    bytes.extend_from_slice(&[5, b'1', b'b', b'i', b't', 0x83]);
    bytes.extend_from_slice(TYPE_OPEN);
    bytes.extend_from_slice(&[0x96, 0xfe]);
    bytes.extend_from_slice(CATALOG_OPEN);

    assert!(parse_runs(&bytes)[0].type_descriptors.is_empty());
}
