use super::*;

pub(super) fn valid_entity_record_shape(record: &CatiaEntityRecord) -> bool {
    if let Some(body) = &record.inline_body() {
        return record.lead == 0x03
            && body.first() == Some(&record.lead)
            && u64::try_from(body.len())
                .ok()
                .and_then(|len| len.checked_add(6))
                == Some(record.byte_len)
            && record.definition_len() == 0
            && record.definition_prefix().is_empty()
            && record.definition_schema_selections.is_empty()
            && record.definition_suffix().is_empty()
            && record.value_len() == 0
            && record.value_payload().is_empty()
            && record.value_fields().is_empty()
            && record.value_schema_selections.is_empty()
            && record.value_packets.is_empty()
            && record.numeric_pair.is_none()
            && record.reference_signature.is_none()
            && record.record_suffix().is_empty()
            && record.suffix_value().is_none()
            && record.suffix_framing().is_none()
            && record.suffix_schema_selection.is_none();
    }
    let Some(definition_body_len) = u64::try_from(record.definition_prefix().len())
        .ok()
        .and_then(|prefix_len| prefix_len.checked_add(5))
        .and_then(|len| {
            u64::try_from(record.definition_suffix().len())
                .ok()
                .and_then(|suffix_len| len.checked_add(suffix_len))
        })
    else {
        return false;
    };
    let Some(value_len) = u64::try_from(record.value_payload().len())
        .ok()
        .and_then(|len| len.checked_add(6))
    else {
        return false;
    };
    let Some(total_len) = 7_u64
        .checked_add(u64::from(record.definition_len()))
        .and_then(|len| len.checked_add(u64::from(record.value_len())))
        .and_then(|len| {
            u64::try_from(record.record_suffix().len())
                .ok()
                .and_then(|suffix_len| len.checked_add(suffix_len))
        })
    else {
        return false;
    };
    u64::from(record.definition_len()) == definition_body_len + 6
        && u64::from(record.value_len()) == value_len
        && record.byte_len == total_len
        && record.value_fields() == value_block::tokenize(record.value_payload())
        && record.value_packets
            == entity_table::value_packets(record.value_payload(), &record.value_fields())
        && record.numeric_pair == entity_table::parse_numeric_pair(record.value_payload())
        && record
            .reference_signature
            .as_ref()
            .map(|signature| &signature.production)
            == entity_table::parse_reference_signature(record.value_payload()).as_ref()
        && record.suffix_value() == entity_suffix_value(record.record_suffix()).as_ref()
}

pub(super) fn legacy_schema_identifiers(
    program: &CatiaLegacySchemaProgram,
) -> Option<Vec<CatiaLegacySchemaIdentifier>> {
    let program_offset = usize::try_from(program.byte_offset).ok()?;
    Some(
        legacy_entity::parse_schema_identifiers(&program.data, program_offset)
            .into_iter()
            .map(|identifier| CatiaLegacySchemaIdentifier {
                byte_offset: identifier.offset as u64,
                value: identifier.value,
            })
            .collect(),
    )
}

pub(super) fn legacy_value_name(
    roles: &[CatiaLegacyRoleSelector],
    fields: &[CatiaLegacyTextField],
    entity_id: u32,
    value_offset: u64,
) -> Option<(u64, String)> {
    let mut literal_names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("name"))
    });
    if let Some(name) = literal_names.next() {
        if literal_names.next().is_none() {
            return Some((name.byte_offset, name.value.clone()));
        }
        return None;
    }

    let name = legacy_evaluated_value_name(roles, fields, entity_id, value_offset)?;
    Some((name.byte_offset, name.value.clone()))
}

fn legacy_schema_boundary_closes_text(
    run: &CatiaLegacyEntityRun,
    field: &CatiaLegacySchemaField,
    role: &CatiaLegacyRoleSelector,
) -> bool {
    run.text_fields.iter().any(|text| {
        text.byte_offset == field.byte_offset
            && text.entity_id == field.entity_id
            && text.encoding == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
            && text.role.as_ref() == Some(role)
            && text
                .value
                .len()
                .checked_add(1)
                .and_then(|length| u8::try_from(length).ok())
                .is_some_and(|length| {
                    field.payload.first() == Some(&length)
                        && field.payload.get(1..) == Some(text.value.as_bytes())
                })
    })
}

fn valid_legacy_relation(run: &CatiaLegacyEntityRun, relation: &CatiaLegacyRelation) -> bool {
    let Some(parsed) = legacy_entity::parse_relation_signature(&relation.type_signature) else {
        return false;
    };
    let Some(expression_field) = run.text_fields.iter().find(|field| {
        field.entity_id == relation.entity_id
            && field.byte_offset == relation.expression_offset
            && field.value == relation.expression
    }) else {
        return false;
    };
    let Some(signature_field) = run.text_fields.iter().find(|field| {
        field.entity_id == relation.entity_id
            && field.byte_offset == relation.signature_offset
            && field.value == relation.type_signature
    }) else {
        return false;
    };
    let parameter_entity_id = expression_field
        .role
        .as_ref()
        .zip(signature_field.role.as_ref())
        .and_then(|(owner, parameter)| {
            (owner.name.literal() == Some("body")
                && owner.selector == relation.entity_id
                && parameter.name.literal() == Some("param")
                && run
                    .identities
                    .iter()
                    .any(|identity| identity.entity_id == parameter.selector))
            .then_some(parameter.selector)
        });
    let body_selector = expression_field
        .role
        .as_ref()
        .filter(|role| role.name.literal() == Some("body"))
        .map(|role| role.selector);
    let parameter_selector = signature_field
        .role
        .as_ref()
        .filter(|role| role.name.literal() == Some("param"))
        .map(|role| role.selector);
    valid_legacy_relation_field_pair(run, expression_field, signature_field)
        && relation.body_selector == body_selector
        && relation.parameter_selector == parameter_selector
        && relation.parameter_entity_id == parameter_entity_id
        && (relation.result_type == "VoidType") == relation.output.is_some()
        && parsed.result_type() == relation.result_type
        && parsed.inputs.len() == relation.inputs.len()
        && parsed
            .inputs
            .iter()
            .zip(&relation.inputs)
            .all(|(parsed, stored)| {
                parsed.parameter == stored.parameter && parsed.value_type == stored.value_type
            })
        && parsed
            .output()
            .map(|output| (output.parameter.as_str(), output.value_type.as_str()))
            == relation
                .output
                .as_ref()
                .map(|output| (output.parameter.as_str(), output.value_type.as_str()))
}

fn valid_legacy_relation_field_pair(
    run: &CatiaLegacyEntityRun,
    expression: &CatiaLegacyTextField,
    signature: &CatiaLegacyTextField,
) -> bool {
    let fields = run
        .text_fields
        .iter()
        .filter(|field| field.entity_id == expression.entity_id)
        .collect::<Vec<_>>();
    let mut body_fields = fields.iter().copied().filter(|field| {
        field
            .role
            .as_ref()
            .is_some_and(|role| role.name.literal() == Some("body"))
    });
    let body = body_fields.next();
    let unique_body = body_fields.next().is_none();
    let mut parameter_fields = fields.iter().copied().filter(|field| {
        field
            .role
            .as_ref()
            .is_some_and(|role| role.name.literal() == Some("param"))
    });
    let parameter = parameter_fields.next();
    let unique_parameter = parameter_fields.next().is_none();
    let role_bound = unique_body
        && unique_parameter
        && body == Some(expression)
        && parameter == Some(signature)
        && expression.byte_offset < signature.byte_offset;
    let selected_role_bound = matches!(
        fields.as_slice(),
        [prelude, selected_expression, selected_signature]
            if prelude.value.is_empty()
                && prelude.role.as_ref().is_none_or(|role| {
                    matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                })
                && prelude.encoding == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                && selected_expression.encoding
                    == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                && selected_signature.encoding
                    == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                && selected_expression.role.as_ref().is_some_and(|role| {
                    matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                })
                && selected_signature.role.as_ref().is_some_and(|role| {
                    matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                })
                && *selected_expression == expression
                && *selected_signature == signature
    );
    let complete_pair = matches!(fields.as_slice(), [first, second] if *first == expression && *second == signature);
    role_bound || selected_role_bound || complete_pair
}

pub(super) fn validate_legacy_entity_runs(
    runs: &[CatiaLegacyEntityRun],
    require_field_codes: bool,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut previous_end = None;
    for (index, run) in runs.iter().enumerate() {
        let run_end = run.byte_offset.checked_add(run.byte_len);
        let valid = run.id == format!("catia:legacy:entity-run#{index:08}")
            && run_end == Some(run.catalog_offset)
            && run.schema_program.as_ref().is_none_or(|program| {
                !program.data.is_empty()
                    && program.data.last() == Some(&0xfe)
                    && run.catalog_offset.checked_add(
                        u64::try_from(legacy_entity::SCHEMA_PROGRAM_OFFSET_FROM_CATALOG)
                            .expect("schema-program prefix length fits u64"),
                    ) == Some(program.byte_offset)
                    && u64::try_from(program.data.len())
                        .ok()
                        .and_then(|len| program.byte_offset.checked_add(len))
                        == Some(program.boundary_byte_offset)
                    && legacy_schema_identifiers(program)
                        .is_some_and(|identifiers| program.identifiers == identifiers)
            })
            && previous_end.is_none_or(|end| end <= run.byte_offset)
            && run.identities.first().is_some_and(|identity| {
                identity.byte_offset == run.byte_offset && identity.entity_id == 1
            })
            && run.identities.windows(2).all(|pair| {
                pair[0]
                    .byte_offset
                    .checked_add(6)
                    .is_some_and(|end| end <= pair[1].byte_offset)
                    && pair[0].entity_id < pair[1].entity_id
            })
            && run.identities.iter().all(|identity| {
                matches!(identity.lead, 0x81 | 0x82 | 0xe5 | 0xfd)
                    && identity
                        .byte_offset
                        .checked_add(6)
                        .is_some_and(|end| end <= run.catalog_offset)
            })
            && run
                .role_selectors
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.role_selectors.iter().all(|role| {
                role.byte_offset >= run.byte_offset
                    && role.byte_offset < run.catalog_offset
                    && role.selector != 0
                    && match &role.name {
                        CatiaLegacyRoleName::Literal(name) => valid_legacy_identifier(name),
                        CatiaLegacyRoleName::Selector(selector) => *selector != 0,
                    }
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < role.byte_offset)
                        .is_some_and(|identity| {
                            let interval_end = run
                                .identities
                                .iter()
                                .find(|next| next.byte_offset > identity.byte_offset)
                                .map_or(run.catalog_offset, |next| next.byte_offset);
                            identity.entity_id == role.entity_id
                                && role.end_offset().is_none_or(|end| {
                                    end <= interval_end
                                        && role.field_code.is_none_or(|_| {
                                            end.checked_add(4)
                                                .is_some_and(|field_end| field_end <= interval_end)
                                        })
                                })
                        })
            })
            && run
                .text_fields
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.text_fields.iter().all(|field| {
                (!field.value.is_empty()
                    || field.encoding == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail)
                    && field.value.chars().all(|character| {
                        !character.is_control() || matches!(character, '\t' | '\n' | '\r')
                    })
                    && field.byte_offset >= run.byte_offset
                    && field.byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < field.byte_offset)
                        .is_some_and(|identity| identity.entity_id == field.entity_id)
                    && field.role.as_ref().is_none_or(|role| {
                        role.byte_offset >= run.byte_offset
                            && role.byte_offset < field.byte_offset
                            && role.entity_id == field.entity_id
                            && role.selector != 0
                            && match &role.name {
                                CatiaLegacyRoleName::Literal(name) => valid_legacy_identifier(name),
                                CatiaLegacyRoleName::Selector(selector) => *selector != 0,
                            }
                            && run.role_selectors.contains(role)
                            && role.end_offset().is_none_or(|end| end == field.byte_offset)
                            && (!require_field_codes || role.field_code == Some(0x1200))
                            && run
                                .identities
                                .iter()
                                .rfind(|identity| identity.byte_offset < role.byte_offset)
                                .is_some_and(|identity| identity.entity_id == field.entity_id)
                    })
            })
            && run
                .schema_fields
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.schema_fields.iter().all(|field| {
                field.byte_offset >= run.byte_offset
                    && field.byte_offset < run.catalog_offset
                    && field.boundary_role_byte_offset > field.byte_offset
                    && field.byte_offset.checked_add(4).and_then(|payload_offset| {
                        payload_offset.checked_add(u64::try_from(field.payload.len()).ok()?)
                    }) == Some(field.boundary_role_byte_offset)
                    && run.role_selectors.windows(2).any(|roles| {
                        roles[0].byte_offset == field.role_byte_offset
                            && roles[0].entity_id == field.entity_id
                            && roles[0].end_offset() == Some(field.byte_offset)
                            && (!require_field_codes
                                || roles[0].field_code == Some(field.field_code))
                            && roles[1].byte_offset == field.boundary_role_byte_offset
                            && roles[1].entity_id == field.entity_id
                            && (!require_field_codes
                                || roles[1].field_code.is_some()
                                || legacy_schema_boundary_closes_text(run, field, &roles[0]))
                    })
            })
            && run
                .relations
                .iter()
                .all(|relation| valid_legacy_relation(run, relation))
            && run
                .synchronous_states
                .windows(2)
                .all(|pair| pair[0].role_byte_offset < pair[1].role_byte_offset)
            && run.synchronous_states.iter().all(|state| {
                state.role_byte_offset >= run.byte_offset
                    && state.role_byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < state.role_byte_offset)
                        .is_some_and(|identity| identity.entity_id == state.entity_id)
                    && run
                        .role_selectors
                        .iter()
                        .filter(|role| {
                            role.byte_offset == state.role_byte_offset
                                && role.entity_id == state.entity_id
                                && (role.name.literal() == Some("synchrone")
                                    || (matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                                        && role
                                            .end_offset()
                                            .and_then(|end| end.checked_add(5))
                                            .is_some_and(|next_role_offset| {
                                                run.role_selectors.iter().any(|next| {
                                                    next.entity_id == state.entity_id
                                                        && next.byte_offset == next_role_offset
                                                })
                                            })))
                                && role.selector == state.selector
                        })
                        .count()
                        == 1
            })
            && run
                .type_descriptors
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.type_descriptors.iter().all(|descriptor| {
                descriptor.byte_offset >= run.byte_offset
                    && descriptor.byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < descriptor.byte_offset)
                        .is_some_and(|identity| identity.entity_id == descriptor.entity_id)
                    && match &descriptor.value {
                        CatiaLegacyTypeValue::Name { value } => valid_legacy_identifier(value),
                        CatiaLegacyTypeValue::Selector { value } => *value != 0,
                    }
            })
            && run
                .scalar_values
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.scalar_values.iter().all(|value| {
                value.id == format!("catia:legacy:scalar#{index:08}-{:016}", value.byte_offset)
                    && value.byte_offset >= run.byte_offset
                    && value.byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < value.byte_offset)
                        .is_some_and(|identity| identity.entity_id == value.entity_id)
                    && (value.name.is_none()
                        || run
                            .scalar_values
                            .iter()
                            .filter(|candidate| candidate.entity_id == value.entity_id)
                            .count()
                            == 1)
                    && match (&value.name_field, &value.name) {
                        (Some(name_field), Some(name)) => {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                value.entity_id,
                                value.byte_offset,
                            ) == Some((*name_field, name.clone()))
                        }
                        (None, None) => legacy_value_name(
                            &run.role_selectors,
                            &run.text_fields,
                            value.entity_id,
                            value.byte_offset,
                        )
                        .is_none(),
                        (Some(_), None) | (None, Some(_)) => false,
                    }
                    && match value.evaluation {
                        CatiaLegacyScalarEvaluation::Value { bits } => {
                            f64::from_bits(bits).is_finite()
                        }
                        CatiaLegacyScalarEvaluation::Unset => true,
                    }
            })
            && run
                .string_values
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.string_values.iter().all(|value| {
                value.id == format!("catia:legacy:string#{index:08}-{:016}", value.byte_offset)
                    && value.byte_offset >= run.byte_offset
                    && value.byte_offset < run.catalog_offset
                    && value.value.chars().all(|character| {
                        !character.is_control() || matches!(character, '\t' | '\n' | '\r')
                    })
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < value.byte_offset)
                        .is_some_and(|identity| identity.entity_id == value.entity_id)
                    && (value.name.is_none()
                        || run
                            .string_values
                            .iter()
                            .filter(|candidate| candidate.entity_id == value.entity_id)
                            .count()
                            == 1)
                    && match (&value.name_field, &value.name) {
                        (Some(name_field), Some(name)) => {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                value.entity_id,
                                value.byte_offset,
                            ) == Some((*name_field, name.clone()))
                        }
                        (None, None) => legacy_value_name(
                            &run.role_selectors,
                            &run.text_fields,
                            value.entity_id,
                            value.byte_offset,
                        )
                        .is_none(),
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            })
            && run
                .integer_values
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.integer_values.iter().all(|value| {
                value.id == format!("catia:legacy:integer#{index:08}-{:016}", value.byte_offset)
                    && value.byte_offset >= run.byte_offset
                    && value.byte_offset < run.catalog_offset
                    && match value.encoding {
                        CatiaLegacyIntegerEncoding::Inline => (0..=126).contains(&value.value),
                        CatiaLegacyIntegerEncoding::WideI32 => true,
                    }
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < value.byte_offset)
                        .is_some_and(|identity| identity.entity_id == value.entity_id)
                    && (value.name.is_none()
                        || run
                            .integer_values
                            .iter()
                            .filter(|candidate| candidate.entity_id == value.entity_id)
                            .count()
                            == 1)
                    && match (&value.name_field, &value.name) {
                        (Some(name_field), Some(name)) => {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                value.entity_id,
                                value.byte_offset,
                            ) == Some((*name_field, name.clone()))
                        }
                        (None, None) => legacy_value_name(
                            &run.role_selectors,
                            &run.text_fields,
                            value.entity_id,
                            value.byte_offset,
                        )
                        .is_none(),
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            });
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "legacy entity run `{}` has an invalid identity sequence",
                run.id
            )));
        }
        previous_end = run_end;
    }
    Ok(())
}
