// SPDX-License-Identifier: Apache-2.0
//! Core legacy scans, small source sorts, and lazy field joins.

use super::super::*;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};

#[test]
fn legacy_small_scalar_and_role_sorts_skip_constant_work() {
    for count in 0..=2 {
        let mut scalar_bytes = Vec::new();
        let mut expected = Vec::new();
        let mut role_bytes = Vec::new();
        for index in 0..count {
            expected.push(scalar_bytes.len());
            scalar_bytes.extend_from_slice(if index % 2 == 0 {
                SCALAR_OPEN
            } else {
                NAMED_SCALAR_OPEN
            });
            scalar_bytes.push(0xe7);
            role_bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0xd1, 0]);
        }
        for (operation, is_role) in [
            ("catia_legacy_scalar_sort", false),
            ("catia_legacy_role_sort", true),
        ] {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            crate::test_support::with_work_limit(u64::MAX, |ctx| {
                if is_role {
                    let roles = parse_role_selectors(ctx, &role_bytes, 0, role_bytes.len(), 1)?;
                    assert_eq!(roles.len(), count);
                    for (index, role) in roles.iter().enumerate() {
                        assert_eq!(role.offset, index * 7);
                    }
                } else {
                    let values = parse_scalar_values(ctx, &scalar_bytes, 0, scalar_bytes.len(), 1)?;
                    assert_eq!(
                        values.iter().map(|value| value.offset).collect::<Vec<_>>(),
                        expected
                    );
                    assert!(values
                        .iter()
                        .all(|value| value.evaluation == LegacyScalarEvaluation::Unset));
                }
                assert_eq!(ctx.resource_refusal(), None);
                Ok::<_, cadmpeg_core::CodecError>(())
            })
            .expect("small numeric source order needs no general sort");
        }
    }
    let scalar_bytes = [
        SCALAR_OPEN,
        &[0xe7],
        NAMED_SCALAR_OPEN,
        &[0xe7],
        SCALAR_OPEN,
        &[0xe7],
    ]
    .concat();
    let role_bytes = [5, b'n', b'a', b'm', b'e', 0xd1, 0].repeat(3);
    for (operation, is_role) in [
        ("catia_legacy_scalar_sort", false),
        ("catia_legacy_role_sort", true),
    ] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            if is_role {
                parse_role_selectors(ctx, &role_bytes, 0, role_bytes.len(), 1)
                    .map(|values| values.len())
            } else {
                parse_scalar_values(ctx, &scalar_bytes, 0, scalar_bytes.len(), 1)
                    .map(|values| values.len())
            }
        })
        .expect_err("larger source orders retain sorting admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation)
        );
    }
}

#[test]
fn legacy_role_uniqueness_stops_at_second_matches_before_the_tail() {
    let mut fields = Vec::new();
    for role_name in ["body", "body", "param", "param"] {
        let offset = fields.len();
        fields.push(LegacyTextField {
            offset,
            entity_id: 1,
            encoding: LegacyTextEncoding::U8InclusiveLength,
            role: Some(LegacyRoleSelector {
                offset,
                entity_id: 1,
                name: LegacyRoleName::Literal(role_name.to_owned()),
                encoding: LegacyRoleSelectorEncoding::Paged,
                selector: 1,
                field_code: Some(0x1200),
            }),
            value: "irrelevant".to_owned(),
        });
    }
    for _ in 0..4096 {
        fields.push(LegacyTextField {
            offset: fields.len(),
            entity_id: 1,
            encoding: LegacyTextEncoding::U8InclusiveLength,
            role: None,
            value: "irrelevant".to_owned(),
        });
    }
    // Grouping visits all fields; role searches visit 2 body and 4 param slots.
    // Sixteen extra units include those six visits and empty identity collection.
    let cap = u64::try_from(fields.len() + 16).expect("fixture work");
    assert!(
        crate::test_support::with_work_limit(cap, |ctx| parse_relations(ctx, &fields, &[]))
            .expect("role searches leave the tail unread")
            .is_empty()
    );
    let error = crate::test_support::with_work_refusal("catia_legacy_relation_roles", |ctx| {
        parse_relations(ctx, &fields, &[])
    })
    .expect_err("actual role visits require admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "catia_legacy_relation_roles")
    );
}

#[test]
fn legacy_name_count_backing_is_temporary_through_binding() {
    let mut values = (1..=4096)
        .map(|entity_id| LegacyScalarValue {
            offset: 0,
            entity_id,
            encoding: LegacyScalarEncoding::Standalone85,
            name_offset: None,
            name: None,
            evaluation: LegacyScalarEvaluation::Unset,
        })
        .collect::<Vec<_>>();
    crate::test_support::with_retained_limit(0, |ctx| {
        let names = LegacyNameIndex::new(ctx, &[], &[], &[])?;
        bind_value_names(ctx, &names, &[], &[], &mut values)?;
        assert_eq!(ctx.resource_refusal(), None);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("discarded counts do not consume retained storage");
    for (index, value) in values.iter().enumerate() {
        assert_eq!(
            value.entity_id,
            u32::try_from(index + 1).expect("fixture identity")
        );
        assert_eq!(value.evaluation, LegacyScalarEvaluation::Unset);
        assert_eq!(value.name, None);
        assert_eq!(value.name_offset, None);
    }
}

fn closing_text_fields(count: usize) -> (Vec<u8>, Vec<LegacyRoleSelector>, Vec<LegacyTextField>) {
    let mut data = Vec::new();
    let mut roles = Vec::new();
    let mut fields = Vec::new();
    for _ in 0..count {
        let start = data.len();
        data.extend_from_slice(&[7, b's', b'o', b'u', b'r', b'c', b'e', 0xd1, 0]);
        data.extend_from_slice(TEXT_OPEN);
        data.extend_from_slice(&[2, b'a', 2, 0xe3, 1]);
        let role = LegacyRoleSelector {
            offset: start,
            entity_id: 1,
            name: LegacyRoleName::Literal("source".to_owned()),
            encoding: LegacyRoleSelectorEncoding::Paged,
            selector: 1,
            field_code: Some(0x1200),
        };
        fields.push(LegacyTextField {
            offset: start + 9,
            entity_id: 1,
            encoding: LegacyTextEncoding::U8InclusiveLengthE3RoleTail,
            role: Some(role.clone()),
            value: "a".to_owned(),
        });
        roles.push(role);
        roles.push(LegacyRoleSelector {
            offset: start + 15,
            entity_id: 1,
            name: LegacyRoleName::Selector(2),
            encoding: LegacyRoleSelectorEncoding::Paged,
            selector: 4610,
            field_code: None,
        });
    }
    (data, roles, fields)
}

#[test]
fn legacy_schema_field_text_join_indexes_once_and_admits_role_names() {
    const COUNT: usize = 1024;
    let (data, roles, fields) = closing_text_fields(COUNT);
    let output = crate::test_support::with_work_limit(512 * 1024, |ctx| {
        parse_schema_fields(ctx, &data, &roles, &fields)
    })
    .expect("one text index and one query per unique role offset fit linear work");
    assert_eq!(output.len(), COUNT);
    for (index, field) in output.iter().enumerate() {
        assert_eq!(field.role_offset, index * 18);
        assert_eq!(field.offset, index * 18 + 9);
        assert_eq!(field.boundary_role_offset, index * 18 + 15);
        assert_eq!(field.entity_id, 1);
        assert_eq!(field.field_code, 0x1200);
        assert_eq!(field.payload, [2, b'a']);
    }
    for operation in [
        "catia_legacy_schema_role_pairs",
        "catia_legacy_schema_text_index_visits",
        "catia_legacy_schema_text_index",
        "catia_legacy_schema_text_candidates",
        "catia_legacy_schema_role_name",
    ] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            parse_schema_fields(ctx, &data, &roles, &fields)
        })
        .expect_err(
            "the source walk, index, query and variable name comparison each admit actual work",
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation)
        );
    }
    let (data, roles, mut fields) = closing_text_fields(1);
    fields[0].role.as_mut().expect("bound role").name =
        LegacyRoleName::Literal("different".to_owned());
    assert!(
        crate::test_support::with_service_context(|ctx| parse_schema_fields(
            ctx, &data, &roles, &fields
        ))
        .expect("role mismatch rejects text closure")
        .is_empty()
    );
}

#[test]
fn legacy_schema_field_join_skips_text_index_when_boundaries_bind_fields() {
    let mut data = Vec::new();
    let mut roles = Vec::new();
    for _ in 0..1024 {
        let offset = data.len();
        data.extend_from_slice(&[1, 0xd1, 0, 0xe8, 0x28, 0x17, 0x01, 0xfe]);
        roles.push(LegacyRoleSelector {
            offset,
            entity_id: 1,
            name: LegacyRoleName::Selector(1),
            encoding: LegacyRoleSelectorEncoding::Paged,
            selector: 1,
            field_code: Some(0x1728),
        });
    }
    let (_, _, fields) = closing_text_fields(4096);
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_legacy_schema_text_index_visits",
        None,
    );
    let output = crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let output = parse_schema_fields(ctx, &data, &roles, &fields)?;
        assert_eq!(ctx.resource_refusal(), None);
        Ok::<_, cadmpeg_core::CodecError>(output)
    })
    .expect("source-bound fields do not query the text index");
    assert_eq!(output.len(), 1023);
    for (index, field) in output.iter().enumerate() {
        assert_eq!(field.role_offset, index * 8);
        assert_eq!(field.boundary_role_offset, (index + 1) * 8);
        assert_eq!(field.payload, [0xfe]);
    }
}

#[test]
fn void_relation_literal_admits_storage_without_input_work() {
    let signature = LegacyRelationSignature {
        inputs: Vec::new(),
        result: LegacyRelationResult::Void {
            output: LegacyRelationParameter {
                parameter: "#2_".to_owned(),
                value_type: "Length".to_owned(),
            },
        },
    };
    let (inputs, output, result_type) =
        crate::test_support::with_work_limit(0, |ctx| signature.into_parts(ctx))
            .expect("the fixed result literal has no input-sized work");
    assert!(inputs.is_empty());
    let output = output.expect("void output");
    assert_eq!(output.parameter, "#2_");
    assert_eq!(output.value_type, "Length");
    assert_eq!(result_type, "VoidType");
}
