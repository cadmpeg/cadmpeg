// SPDX-License-Identifier: Apache-2.0
//! Test-only CATIA native decode/load/store helpers.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use cadmpeg_ir::geometry::{knots_nondecreasing, knots_strictly_increasing};

use crate::container;
use crate::entity_table;
use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;
use crate::legacy_entity;
use crate::object_graph;
use crate::value_block;

use super::*;
use super::{
    consolidated_vertex_identities, containing_finjpl_segment, definition_chain_value,
    definition_schema_selections, definition_value, derive_reference_signature_cohorts,
    derive_schema_configuration_row_chains, design_object_id, design_objects, entity_class_index,
    entity_incidences, entity_suffix_schema_selection, entity_suffix_value,
    entity_value_schema_selections, external_reference_views, finjpl_family, formula_relation,
    parameter_value, preview_views, range_interval, reference_signature, relation_expression,
    relation_program_instance, repeated_reference_schema_selection, resolved_constraint_range,
    resolved_payload_references, resolved_storage_link, schema_configuration_record,
    schema_configuration_row_link, semantic_entity_indices, store_projection,
    terminal_null_entity_id, valid_legacy_identifier, value_schema_selections,
    zero_entity_endpoint_locus_candidates, zero_entity_endpoint_pair_candidates,
    zero_entity_record, zero_entity_vertex_owner, CatiaEntityReferenceIndex,
    CATIA_CONFIGURATION_INCIDENCE_VERSION, CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION,
    CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION, CATIA_LEGACY_EVALUATED_VALUE_NAME_VERSION,
    CATIA_LEGACY_IDENTITY_LEAD_VERSION, CATIA_LEGACY_ROLE_FIELD_CODE_VERSION,
    CATIA_LEGACY_ROLE_SELECTOR_VERSION, CATIA_LEGACY_SCHEMA_BOUNDARY_VERSION,
    CATIA_LEGACY_SCHEMA_IDENTIFIER_VERSION, CATIA_OBJECT_GRAPH_SEGMENT_VERSION,
    CATIA_SUFFIX_FRAMING_VERSION, CATIA_TERMINAL_NULL_REFERENCE_VERSION,
    CATIA_TYPED_OWNER_SLOT_VERSION,
};

impl CatiaOwnerPacketPayload {
    fn final_reference(&self) -> Option<u32> {
        match self {
            Self::FixedNine { references, .. } => references.last().copied(),
            Self::Counted { references, .. } => references.last().copied(),
        }
    }
}

fn valid_entity_record_shape(record: &CatiaEntityRecord) -> bool {
    if let Some(body) = &record.inline_body {
        return record.lead == 0x03
            && body.first() == Some(&record.lead)
            && u64::try_from(body.len())
                .ok()
                .and_then(|len| len.checked_add(6))
                == Some(record.byte_len)
            && record.definition_len == 0
            && record.definition_prefix.is_empty()
            && record.definition_schema_selections.is_empty()
            && record.definition_suffix.is_empty()
            && record.value_len == 0
            && record.value_payload.is_empty()
            && record.value_fields.is_empty()
            && record.value_schema_selections.is_empty()
            && record.value_packets.is_empty()
            && record.numeric_pair.is_none()
            && record.reference_signature.is_none()
            && record.record_suffix.is_empty()
            && record.suffix_value.is_none()
            && record.suffix_framing.is_none()
            && record.suffix_schema_selection.is_none();
    }
    let Some(definition_body_len) = u64::try_from(record.definition_prefix.len())
        .ok()
        .and_then(|prefix_len| prefix_len.checked_add(5))
        .and_then(|len| {
            u64::try_from(record.definition_suffix.len())
                .ok()
                .and_then(|suffix_len| len.checked_add(suffix_len))
        })
    else {
        return false;
    };
    let Some(value_len) = u64::try_from(record.value_payload.len())
        .ok()
        .and_then(|len| len.checked_add(6))
    else {
        return false;
    };
    let Some(total_len) = 7_u64
        .checked_add(u64::from(record.definition_len))
        .and_then(|len| len.checked_add(u64::from(record.value_len)))
        .and_then(|len| {
            u64::try_from(record.record_suffix.len())
                .ok()
                .and_then(|suffix_len| len.checked_add(suffix_len))
        })
    else {
        return false;
    };
    u64::from(record.definition_len) == definition_body_len + 6
        && u64::from(record.value_len) == value_len
        && record.byte_len == total_len
        && record.value_fields == value_block::tokenize(&record.value_payload)
        && record.value_packets
            == entity_table::value_packets(&record.value_payload, &record.value_fields)
        && record.numeric_pair == entity_table::parse_numeric_pair(&record.value_payload)
        && record
            .reference_signature
            .as_ref()
            .map(|signature| &signature.production)
            == entity_table::parse_reference_signature(&record.value_payload).as_ref()
        && record.suffix_value == entity_suffix_value(&record.record_suffix)
}

fn legacy_schema_identifiers(
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

fn legacy_value_name(
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
        && parsed.result_type == relation.result_type
        && parsed.inputs.len() == relation.inputs.len()
        && parsed
            .inputs
            .iter()
            .zip(&relation.inputs)
            .all(|(parsed, stored)| {
                parsed.parameter == stored.parameter && parsed.value_type == stored.value_type
            })
        && parsed
            .output
            .as_ref()
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

fn validate_legacy_entity_runs(
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

fn validate_consolidated_class61_records(
    records: &[CatiaConsolidatedClass61Record],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, record) in records.iter().enumerate() {
        let expected_id = format!("catia:consolidated:class61-record#{index}");
        let valid_payload = match &record.payload {
            CatiaConsolidatedClass61Payload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty() && tail.last() == Some(&0x03)
            }
            CatiaConsolidatedClass61Payload::Long {
                members, scalar, ..
            } => {
                scalar.is_finite()
                    && !members.is_empty()
                    && members.windows(2).all(|pair| pair[0] < pair[1])
            }
        };
        if record.id != expected_id
            || !valid_payload
            || index > 0 && records[index - 1].byte_offset >= record.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated class-0x61 record `{}` is structurally invalid",
                record.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_groups(
    groups: &[CatiaConsolidatedGroup],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, group) in groups.iter().enumerate() {
        let expected_id = format!("catia:consolidated:group#{index}");
        if group.id != expected_id
            || index > 0 && groups[index - 1].byte_offset >= group.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated group `{}` is structurally invalid",
                group.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_cone_faces(
    faces: &[CatiaConsolidatedConeFace],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let points_by_id = parameter_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<HashMap<_, _>>();
    for (index, face) in faces.iter().enumerate() {
        let mut expected_point_offset = face.byte_offset.checked_add(face.byte_len);
        let parameter_run_valid = face.parameter_points.iter().all(|id| {
            match (expected_point_offset, points_by_id.get(id.as_str())) {
                (Some(expected), Some(point)) if point.byte_offset == expected => {
                    expected_point_offset = point.byte_offset.checked_add(point.byte_len);
                    expected_point_offset.is_some()
                }
                _ => false,
            }
        });
        let frame_overhead = face
            .byte_len
            .checked_sub(u64::try_from(face.program.len()).unwrap_or(u64::MAX));
        if face.id != format!("catia:consolidated:cone-face#{index}")
            || face.program.len() < 16
            || face.program.first() != Some(&0x85)
            || !face.program.ends_with(&[0x03, 0x11])
            || !matches!(frame_overhead, Some(21..=23))
            || !face.angular_scale.is_finite()
            || face.half_angle <= 0.0
            || face.half_angle >= std::f64::consts::FRAC_PI_2
            || !parameter_run_valid
            || index > 0 && faces[index - 1].byte_offset >= face.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cone-face descriptor `{}` is structurally invalid",
                face.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_pcurves(
    pcurves: &[CatiaConsolidatedPcurve],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, pcurve) in pcurves.iter().enumerate() {
        let expected_id = format!("catia:consolidated:pcurve#{index}");
        let count = pcurve.knots.len();
        if pcurve.id != expected_id
            || pcurve.degree != 5
            || count < 2
            || pcurve.points.len() != count
            || pcurve.first_derivatives.len() != count
            || pcurve.second_derivatives.len() != count
            || !knots_strictly_increasing(&pcurve.knots)
            || pcurve.range[0] >= pcurve.range[1]
            || pcurve
                .knots
                .iter()
                .chain(pcurve.points.iter().flatten())
                .chain(pcurve.first_derivatives.iter().flatten())
                .chain(pcurve.second_derivatives.iter().flatten())
                .chain(&pcurve.range)
                .any(|value| !value.is_finite())
            || !matches!(pcurve.tail.as_slice(), [0x07] | [0x07, 0x00])
            || index > 0 && pcurves[index - 1].byte_offset >= pcurve.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated pcurve `{}` is structurally invalid",
                pcurve.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_circles(
    circles: &[CatiaConsolidatedCircle],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, circle) in circles.iter().enumerate() {
        let full_circle =
            crate::families::b2::records::circle_range_is_full_turn(circle.radius, circle.range);
        let compact_len = usize::from(circle.layout).checked_sub(5 * size_of::<f64>() + 9);
        let record_id_fits_layout = matches!(
            (compact_len, circle.record_id),
            (Some(1), 0..=63) | (Some(2), 0..=255) | (Some(3), 0..=65_535)
        );
        if circle.id != format!("catia:consolidated:circle#{index}")
            || !(0x32..=0x34).contains(&circle.layout)
            || !record_id_fits_layout
            || circle
                .center_pair
                .iter()
                .chain(&circle.range)
                .chain(&[circle.radius, circle.chart_shift])
                .any(|value| !value.is_finite())
            || circle.center_pair.iter().any(|value| value.abs() > 1e6)
            || circle.radius <= 0.0
            || circle.range[0] >= circle.range[1]
            || circle.full_circle != full_circle
            || index > 0 && circles[index - 1].byte_offset >= circle.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated circle `{}` is structurally invalid",
                circle.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_cones(
    cones: &[CatiaConsolidatedCone],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, cone) in cones.iter().enumerate() {
        let expected_id = format!("catia:consolidated:cone#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            cone.direction_x[1] * cone.direction_y[2] - cone.direction_x[2] * cone.direction_y[1],
            cone.direction_x[2] * cone.direction_y[0] - cone.direction_x[0] * cone.direction_y[2],
            cone.direction_x[0] * cone.direction_y[1] - cone.direction_x[1] * cone.direction_y[0],
        ];
        if cone.id != expected_id
            || cone
                .apex
                .iter()
                .chain(&cone.direction_x)
                .chain(&cone.direction_y)
                .chain(&cone.axis)
                .chain(&[
                    cone.half_angle,
                    cone.pre_angular_range_scalar,
                    cone.angular_range[0],
                    cone.angular_range[1],
                    cone.slant_range[0],
                    cone.slant_range[1],
                    cone.angular_scale,
                    cone.angular_domain[0],
                    cone.angular_domain[1],
                ])
                .any(|value| !value.is_finite())
            || [cone.direction_x, cone.direction_y, cone.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-9)
            || cross
                .iter()
                .zip(cone.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-9)
            || cone.half_angle <= 0.0
            || cone.half_angle >= std::f64::consts::FRAC_PI_2
            || !crate::analytic::periodic_angular_range_is_valid(
                cone.angular_range,
                cone.angular_domain,
            )
            || cone.slant_range[0] < 0.0
            || cone.slant_range[0] >= cone.slant_range[1]
            || cone.angular_scale <= 0.0
            || index > 0 && cones[index - 1].byte_offset >= cone.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cone `{}` is structurally invalid",
                cone.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_cylinders(
    cylinders: &[CatiaConsolidatedCylinder],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, cylinder) in cylinders.iter().enumerate() {
        let expected_id = format!("catia:consolidated:cylinder#{index}");
        let squared_length =
            |direction: [f64; 3]| direction.iter().map(|value| value * value).sum::<f64>();
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let payload_valid = match &cylinder.payload {
            CatiaConsolidatedCylinderPayload::Resolved {
                frame_token,
                axis,
                reference_direction,
            } => {
                let frame_matches_layout = match cylinder.layout {
                    0x52 => {
                        *frame_token == 0x1d
                            && *axis == [1.0, 0.0, 0.0]
                            && *reference_direction == [0.0, 1.0, 0.0]
                    }
                    0x5a => {
                        matches!(*frame_token, 0x19 | 0x1c)
                            && axis[2] == 0.0
                            && *reference_direction == [-axis[1], axis[0], 0.0]
                    }
                    _ => false,
                };
                frame_matches_layout
                    && axis
                        .iter()
                        .chain(reference_direction)
                        .all(|value| value.is_finite())
                    && (squared_length(*axis) - 1.0).abs() <= 1e-9
                    && (squared_length(*reference_direction) - 1.0).abs() <= 1e-9
                    && dot(*axis, *reference_direction).abs() <= 1e-9
                    && crate::families::b2::records::circle_range_is_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
            }
            CatiaConsolidatedCylinderPayload::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            } => {
                cylinder.layout == 0x62
                    && stored_vector
                        .iter()
                        .chain(std::iter::once(range_origin))
                        .all(|value| value.is_finite())
                    && (stored_vector[0].hypot(stored_vector[1]) - 1.0).abs() <= 1e-9
                    && *axis == [0.0, 1.0, 0.0]
                    && *reference_direction == [stored_vector[0], 0.0, stored_vector[1]]
                    && crate::families::b2::records::circle_range_is_within_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
                    && range_origin.to_bits()
                        == crate::families::b2::records::cylinder_range_origin(
                            cylinder.radius,
                            cylinder.u_range,
                        )
                        .to_bits()
            }
        };
        if cylinder.id != expected_id
            || cylinder
                .origin
                .iter()
                .chain(&cylinder.u_range)
                .chain(&cylinder.v_range)
                .chain(&[cylinder.radius])
                .any(|value| !value.is_finite())
            || cylinder.radius <= 0.0
            || cylinder.u_range[0] >= cylinder.u_range[1]
            || cylinder.v_range[0] >= cylinder.v_range[1]
            || !payload_valid
            || index > 0 && cylinders[index - 1].byte_offset >= cylinder.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cylinder `{}` is structurally invalid",
                cylinder.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_embedded_cylinders(
    cylinders: &[CatiaConsolidatedEmbeddedCylinder],
    groups: &[CatiaConsolidatedGroup],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let groups = groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            (
                group.id.as_str(),
                (group, groups.get(index + 1).map(|next| next.byte_offset)),
            )
        })
        .collect::<HashMap<_, _>>();
    for (index, cylinder) in cylinders.iter().enumerate() {
        let squared_length =
            |direction: [f64; 3]| direction.iter().map(|value| value * value).sum::<f64>();
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let group_valid =
            groups
                .get(cylinder.group.as_str())
                .is_some_and(|(group, next_offset)| {
                    group.group_type == 3
                        && group.byte_offset < cylinder.byte_offset
                        && next_offset.is_none_or(|next| cylinder.byte_offset < next)
                });
        if cylinder.id != format!("catia:consolidated:embedded-cylinder#{index}")
            || !group_valid
            || !cylinder
                .origin
                .iter()
                .chain(&cylinder.u_range)
                .chain(&cylinder.v_range)
                .chain(&cylinder.axis)
                .chain(&cylinder.reference_direction)
                .chain(&[cylinder.radius])
                .all(|value| value.is_finite())
            || cylinder.radius <= 0.0
            || cylinder.u_range[0] >= cylinder.u_range[1]
            || cylinder.v_range[0] >= cylinder.v_range[1]
            || !matches!(cylinder.frame_token, 0x19 | 0x1c)
            || cylinder.axis[2] != 0.0
            || cylinder.reference_direction != [-cylinder.axis[1], cylinder.axis[0], 0.0]
            || (squared_length(cylinder.axis) - 1.0).abs() > 1e-9
            || (squared_length(cylinder.reference_direction) - 1.0).abs() > 1e-9
            || dot(cylinder.axis, cylinder.reference_direction).abs() > 1e-9
            || !crate::families::b2::records::circle_range_is_full_turn(
                cylinder.radius,
                cylinder.u_range,
            )
            || index > 0 && cylinders[index - 1].byte_offset >= cylinder.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated embedded cylinder `{}` is structurally invalid",
                cylinder.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_parameter_points(
    points: &[CatiaConsolidatedParameterPoint],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, point) in points.iter().enumerate() {
        let payload_valid = match &point.payload {
            CatiaConsolidatedParameterPointPayload::Uv { uv } => {
                point.layout == 0x12 && uv.iter().all(|value| value.is_finite())
            }
            CatiaConsolidatedParameterPointPayload::StationUv { station, uv } => {
                point.layout == 0x1a
                    && station.is_finite()
                    && uv.iter().all(|value| value.is_finite())
            }
            CatiaConsolidatedParameterPointPayload::FiveScalars { values } => {
                point.layout == 0x2a && values.iter().all(|value| value.is_finite())
            }
        };
        let frame_overhead = point.byte_len.checked_sub(u64::from(point.layout));
        if point.id != format!("catia:consolidated:parameter-point#{index}")
            || !matches!(frame_overhead, Some(5..=7))
            || !matches!(point.prefix, 0x05 | 0x09 | 0x0d | 0x11)
            || !payload_valid
            || index > 0 && points[index - 1].byte_offset >= point.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated parameter point `{}` is structurally invalid",
                point.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_plane_carriers(
    carriers: &[CatiaConsolidatedPlaneCarrier],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, carrier) in carriers.iter().enumerate() {
        let (selector, scalar_count, payload_valid) = match &carrier.payload {
            CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
                point,
                direction,
                tail,
            } => (
                0xe4,
                7,
                point
                    .iter()
                    .chain(direction)
                    .chain(tail)
                    .all(|value| value.is_finite()),
            ),
            CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
                point,
                direction,
                tail,
            } => (
                0xc4,
                8,
                point
                    .iter()
                    .chain(direction)
                    .chain(tail)
                    .all(|value| value.is_finite()),
            ),
            CatiaConsolidatedPlaneCarrierPayload::PointTail { point, tail } => (
                0xec,
                6,
                point.iter().chain(tail).all(|value| value.is_finite()),
            ),
            CatiaConsolidatedPlaneCarrierPayload::ScalarLane { values } => (
                carrier.selector,
                values.len(),
                !values.is_empty() && values.iter().all(|value| value.is_finite()),
            ),
        };
        let header_limit = 1u32.checked_shl(8 * u32::from(carrier.width));
        let scalar_count = u64::try_from(scalar_count).map_err(|_| {
            cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated plane carrier `{}` has too many scalars",
                carrier.id
            ))
        })?;
        let expected_len = 4 + u64::from(carrier.width) + 2 + 8 * scalar_count;
        if carrier.id != format!("catia:consolidated:plane-carrier#{index}")
            || !matches!(carrier.width, 1..=3)
            || header_limit.is_none_or(|limit| carrier.header_token >= limit)
            || !matches!(carrier.flag, 0x03 | 0x13 | 0x83)
            || matches!(
                &carrier.payload,
                CatiaConsolidatedPlaneCarrierPayload::ScalarLane { .. }
            ) && matches!(carrier.selector, 0xe4 | 0xc4 | 0xec)
            || carrier.selector != selector
            || carrier.byte_len != expected_len
            || !payload_valid
            || index > 0 && carriers[index - 1].byte_offset >= carrier.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated plane carrier `{}` is structurally invalid",
                carrier.id
            )));
        }
    }
    Ok(())
}

fn valid_consolidated_plane_geometry(payload: &CatiaConsolidatedPlaneCarrierPayload) -> bool {
    let (point, direction, tail) = match payload {
        CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
            point,
            direction,
            tail,
        } => (*point, [direction[0], direction[1], 0.0], *tail),
        CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
            point,
            direction,
            tail,
        } => (*point, *direction, *tail),
        CatiaConsolidatedPlaneCarrierPayload::PointTail { .. } => return false,
        CatiaConsolidatedPlaneCarrierPayload::ScalarLane { .. } => return false,
    };
    let finite = point
        .iter()
        .chain(direction.iter())
        .chain(tail.iter())
        .all(|value| value.is_finite());
    let norm = direction[0].hypot(direction[1]).hypot(direction[2]);
    finite
        && (norm - 1.0).abs() <= 1e-9
        && direction[2].abs() <= 1e-9
        && tail[0] > 0.0
        && tail[1] < tail[2]
}

fn validate_consolidated_reference_lists(
    lists: &[CatiaConsolidatedReferenceList],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, list) in lists.iter().enumerate() {
        if list.id != format!("catia:consolidated:reference-list#{index}")
            || list.references.is_empty()
            || index > 0 && lists[index - 1].byte_offset >= list.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated reference list `{}` is structurally invalid",
                list.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_revolutions(
    revolutions: &[CatiaConsolidatedRevolution],
    circles: &[CatiaConsolidatedCircle],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, revolution) in revolutions.iter().enumerate() {
        let mut profile_candidates = circles.iter().filter(|circle| {
            circle.range[0].to_bits() == revolution.profile_range[0].to_bits()
                && circle.range[1].to_bits() == revolution.profile_range[1].to_bits()
        });
        let expected_profile = profile_candidates.next().and_then(|circle| {
            profile_candidates
                .next()
                .is_none()
                .then_some(circle.id.as_str())
        });
        let expected_id = format!("catia:consolidated:revolution#{index}");
        let squared_length = |direction: [f64; 3]| {
            direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>()
        };
        let cross = [
            revolution.direction_x[1] * revolution.direction_y[2]
                - revolution.direction_x[2] * revolution.direction_y[1],
            revolution.direction_x[2] * revolution.direction_y[0]
                - revolution.direction_x[0] * revolution.direction_y[2],
            revolution.direction_x[0] * revolution.direction_y[1]
                - revolution.direction_x[1] * revolution.direction_y[0],
        ];
        if revolution.id != expected_id
            || !matches!(revolution.reference_token, 0x08 | 0x0a)
            || revolution.profile_allocation_id == 0
            || revolution
                .origin
                .iter()
                .chain(&revolution.direction_x)
                .chain(&revolution.direction_y)
                .chain(&revolution.axis)
                .chain(&revolution.angular_range)
                .chain(&revolution.profile_range)
                .chain(&[revolution.angular_scale])
                .any(|value| !value.is_finite())
            || revolution.angular_scale <= 0.0
            || revolution.angular_range[0] >= revolution.angular_range[1]
            || revolution.profile_range[0] >= revolution.profile_range[1]
            || revolution.profile_circle.as_deref() != expected_profile
            || [
                revolution.direction_x,
                revolution.direction_y,
                revolution.axis,
            ]
            .into_iter()
            .any(|direction| (squared_length(direction) - 1.0).abs() > 1e-12)
            || cross
                .iter()
                .zip(revolution.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
            || revolution.angular_range[0] / revolution.angular_scale != 0.5
            || (revolution.angular_range[1] - revolution.angular_range[0])
                / revolution.angular_scale
                != std::f64::consts::TAU
            || index > 0 && revolutions[index - 1].byte_offset >= revolution.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated revolution `{}` is structurally invalid",
                revolution.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_line_profiles(
    lines: &[CatiaConsolidatedLineProfile],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, line) in lines.iter().enumerate() {
        let squared_length = line
            .direction
            .iter()
            .map(|component| component * component)
            .sum::<f64>();
        if line.id != format!("catia:consolidated:line-profile#{index}")
            || line
                .origin
                .iter()
                .chain(&line.direction)
                .chain(&line.range)
                .any(|value| !value.is_finite())
            || (squared_length - 1.0).abs() > 1e-12
            || line.range[0] >= line.range[1]
            || index > 0 && lines[index - 1].byte_offset >= line.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated line profile `{}` is structurally invalid",
                line.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_spheres(
    spheres: &[CatiaConsolidatedSphere],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, sphere) in spheres.iter().enumerate() {
        let expected_id = format!("catia:consolidated:sphere#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            sphere.direction_x[1] * sphere.direction_y[2]
                - sphere.direction_x[2] * sphere.direction_y[1],
            sphere.direction_x[2] * sphere.direction_y[0]
                - sphere.direction_x[0] * sphere.direction_y[2],
            sphere.direction_x[0] * sphere.direction_y[1]
                - sphere.direction_x[1] * sphere.direction_y[0],
        ];
        if sphere.id != expected_id
            || sphere
                .center
                .iter()
                .chain(&sphere.direction_x)
                .chain(&sphere.direction_y)
                .chain(&sphere.axis)
                .chain(&sphere.azimuth_range)
                .chain(&sphere.latitude_range)
                .chain(&[sphere.radius])
                .any(|value| !value.is_finite())
            || [sphere.direction_x, sphere.direction_y, sphere.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-12)
            || dot(sphere.direction_x, sphere.direction_y).abs() > 1e-12
            || dot(sphere.direction_x, sphere.axis).abs() > 1e-12
            || dot(sphere.direction_y, sphere.axis).abs() > 1e-12
            || cross
                .iter()
                .zip(sphere.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
            || sphere.radius <= 0.0
            || !crate::analytic::sphere_angular_ranges_are_valid(
                sphere.azimuth_range,
                sphere.latitude_range,
            )
            || index > 0 && spheres[index - 1].byte_offset >= sphere.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated sphere `{}` is structurally invalid",
                sphere.id
            )));
        }
    }
    Ok(())
}

fn validate_consolidated_tori(
    tori: &[CatiaConsolidatedTorus],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, torus) in tori.iter().enumerate() {
        let expected_id = format!("catia:consolidated:torus#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            torus.direction_x[1] * torus.direction_y[2]
                - torus.direction_x[2] * torus.direction_y[1],
            torus.direction_x[2] * torus.direction_y[0]
                - torus.direction_x[0] * torus.direction_y[2],
            torus.direction_x[0] * torus.direction_y[1]
                - torus.direction_x[1] * torus.direction_y[0],
        ];
        if torus.id != expected_id
            || torus
                .center
                .iter()
                .chain(&torus.direction_x)
                .chain(&torus.direction_y)
                .chain(&torus.axis)
                .chain(&torus.major_angular_range)
                .chain(&torus.major_angular_domain)
                .chain(&torus.minor_angular_range)
                .chain(&torus.minor_angular_domain)
                .chain(&[
                    torus.major_radius,
                    torus.minor_radius,
                    torus.major_scale,
                    torus.minor_scale,
                ])
                .any(|value| !value.is_finite())
            || [torus.direction_x, torus.direction_y, torus.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-12)
            || dot(torus.direction_x, torus.direction_y).abs() > 1e-12
            || dot(torus.direction_x, torus.axis).abs() > 1e-12
            || dot(torus.direction_y, torus.axis).abs() > 1e-12
            || cross
                .iter()
                .zip(torus.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
            || torus.major_radius <= 0.0
            || torus.minor_radius <= 0.0
            || !crate::analytic::periodic_angular_range_is_valid(
                torus.major_angular_range,
                torus.major_angular_domain,
            )
            || !crate::analytic::periodic_angular_range_is_valid(
                torus.minor_angular_range,
                torus.minor_angular_domain,
            )
            || torus.major_scale <= 0.0
            || torus.minor_scale <= 0.0
            || index > 0 && tori[index - 1].byte_offset >= torus.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated torus `{}` is structurally invalid",
                torus.id
            )));
        }
    }
    Ok(())
}

fn validate_zero_entity_support_runs(
    runs: &[CatiaZeroEntitySupportRun],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let face_count = runs.iter().filter(|run| run.face.is_some()).count();
    let face_roster_valid = face_count == 0 || face_count == runs.len();
    let expected_loop_count = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .map(|face| face.loop_terminals.len())
        .sum::<usize>();
    let loops = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .flat_map(|face| &face.loops)
        .collect::<Vec<_>>();
    let loop_roster_valid = loops.is_empty()
        || loops.len() == expected_loop_count
            && loops
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset);
    for (index, run) in runs.iter().enumerate() {
        let support_bindings_valid = run.face.as_ref().is_none_or(|face| {
            let binding_count = face
                .loops
                .iter()
                .filter(|loop_record| !loop_record.support_record_ordinals.is_empty())
                .count();
            if binding_count == 0 {
                return true;
            }
            if binding_count != face.loops.len() {
                return false;
            }
            let mut bound = HashSet::new();
            face.loops.iter().all(|loop_record| {
                loop_record
                    .member_ids
                    .iter()
                    .zip(&loop_record.support_record_ordinals)
                    .all(|(member, record_ordinal)| {
                        let slot = loop_record.terminal_id.checked_sub(*member);
                        bound.insert(*record_ordinal)
                            && run.supports.iter().any(|support| {
                                support.record_ordinal == *record_ordinal
                                    && Some(support.face_local_slot) == slot
                            })
                    })
            }) && bound.len() == run.supports.len()
        });
        let face_valid = run.face.as_ref().is_none_or(|face| {
            let derived_terminals = face.allocations.first().and_then(|first| {
                face.allocations[1..]
                    .iter()
                    .map(|allocation| first.checked_sub(*allocation))
                    .collect::<Option<Vec<_>>>()
            });
            let expected_length = face
                .allocations
                .len()
                .checked_mul(5)
                .and_then(|length| length.checked_add(14));
            face.tag[0] == 0x5f
                && face.allocations.len() >= 2
                && !face.allocations.contains(&0)
                && !face.loop_terminals.contains(&0)
                && face.loop_terminals[1..]
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && matches!(face.terminal_control, 0x03 | 0x05)
                && expected_length == Some(usize::from(face.tag[1]) + 12)
                && derived_terminals.as_ref() == Some(&face.loop_terminals)
                && (face.loops.is_empty()
                    || face.loops.len() == face.loop_terminals.len()
                        && face
                            .loops
                            .first()
                            .is_some_and(|outer| matches!(outer.loop_class, 0x41 | 0xc1))
                        && face.loops[1..].iter().all(|inner| inner.loop_class == 0x50)
                        && face.loops.iter().zip(&face.loop_terminals).all(
                            |(loop_record, terminal)| {
                                let edge_count = loop_record.member_ids.len();
                                let reference_count = edge_count
                                    .checked_mul(2)
                                    .and_then(|count| count.checked_add(1));
                                let packed_length = edge_count
                                    .checked_mul(3)
                                    .and_then(|bits| bits.checked_add(7));
                                let expected_length = reference_count.zip(packed_length).and_then(
                                    |(reference_count, packed_length)| {
                                        reference_count
                                            .checked_mul(5)?
                                            .checked_add(16 + packed_length / 8)
                                    },
                                );
                                loop_record.tag[0] == 0x62
                                    && !loop_record.member_ids.is_empty()
                                    && loop_record.typed_references.len() == edge_count
                                    && !loop_record.typed_references.contains(&0)
                                    && (loop_record.typed_records.is_empty()
                                        || loop_record.typed_records.len() == edge_count
                                            && loop_record
                                                .typed_references
                                                .iter()
                                                .zip(&loop_record.typed_records)
                                                .all(|(ordinal, id)| {
                                                    zero_entity_record(records, *ordinal)
                                                        .is_some_and(|record| &record.id == id)
                                                }))
                                    && (loop_record.support_record_ordinals.is_empty()
                                        || loop_record.support_record_ordinals.len() == edge_count)
                                    && loop_record.forward_senses.len() == edge_count
                                    && {
                                        let endpoints = loop_record
                                            .support_record_ordinals
                                            .iter()
                                            .map(|ordinal| {
                                                run.supports
                                                    .iter()
                                                    .find(|support| {
                                                        support.record_ordinal == *ordinal
                                                    })
                                                    .and_then(|support| support.model_endpoints)
                                            })
                                            .collect::<Vec<_>>();
                                        let expected = crate::families::zero_entity::records::
                                            oriented_closed_model_endpoints(
                                                &endpoints,
                                                &loop_record.forward_senses,
                                            )
                                            .unwrap_or_default();
                                        loop_record.oriented_model_endpoints == expected
                                    }
                                    && loop_record.terminal_id == *terminal
                                    && loop_record.gap != 0
                                    && matches!(loop_record.loop_class, 0x41 | 0x50 | 0xc1)
                                    && loop_record.member_ids.iter().enumerate().all(
                                        |(member_index, member)| {
                                            u32::try_from(member_index).ok().and_then(
                                                |member_index| {
                                                    loop_record
                                                        .terminal_id
                                                        .checked_sub(loop_record.gap)?
                                                        .checked_sub(member_index)
                                                },
                                            ) == Some(*member)
                                        },
                                    )
                                    && expected_length == Some(usize::from(loop_record.tag[1]) + 12)
                                    && zero_entity_record(records, loop_record.record_ordinal)
                                        .is_some_and(|record| {
                                            record.byte_offset == loop_record.byte_offset
                                                && record.tag == loop_record.tag
                                        })
                            },
                        ))
                && zero_entity_record(records, face.record_ordinal).is_some_and(|record| {
                    record.byte_offset == face.byte_offset && record.tag == face.tag
                })
                && support_bindings_valid
                && (index == 0
                    || runs[index - 1]
                        .face
                        .as_ref()
                        .is_none_or(|previous| previous.byte_offset < face.byte_offset))
        });
        let carrier_tag =
            zero_entity_record(records, run.carrier_record_ordinal).map(|record| record.tag);
        let supports_valid = !run.supports.is_empty()
            && run
                .supports
                .iter()
                .enumerate()
                .all(|(support_index, support)| {
                    if support.face_local_slot == 0 {
                        return false;
                    }
                    let endpoints_valid = match (support.tag, support.uv_endpoints) {
                        (
                            [0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8],
                            Some(endpoints),
                        ) => endpoints.iter().flatten().all(|value| value.is_finite()),
                        ([0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8], None) => {
                            false
                        }
                        ([0x21, _], None) => true,
                        _ => false,
                    };
                    let model_endpoints_valid = support.model_endpoints.is_none_or(|endpoints| {
                        support.uv_endpoints.is_some()
                            && endpoints.iter().all(|point| {
                                [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                            })
                    });
                    let model_midpoint_valid = support.model_midpoint.is_none_or(|point| {
                        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                    });
                    let model_curve_valid =
                        validate_zero_entity_model_curve(carrier_tag, support.model_curve.as_ref());
                    let model_curve_construction_valid =
                        validate_zero_entity_model_curve_construction(
                            carrier_tag,
                            support.model_curve.as_ref(),
                            support.model_curve_construction.as_ref(),
                        );
                    let has_model_carrier =
                        support.model_curve.is_some() || support.model_curve_construction.is_some();
                    let has_pcurve = support.pcurve.is_some();
                    let model_parameters_valid =
                        support.model_parameters.is_some_and(|parameters| {
                            parameters.into_iter().all(f64::is_finite)
                                && parameters[0] != parameters[1]
                        }) == has_model_carrier;
                    let pcurve_valid = match (&support.tag, &support.pcurve) {
                        (
                            [0x21, tag @ (0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8)],
                            Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                                degree,
                                knots,
                                control_points,
                                weights,
                                periodic: false,
                            }),
                        ) => {
                            let (
                                expected_degree,
                                expected_controls,
                                expected_multiplicities,
                                rational,
                            ): (u32, usize, &[usize], bool) = match tag {
                                0x45 => (3, 12, &[4, 2, 2, 2, 2, 4], false),
                                0x71 => (1, 2, &[2, 2], false),
                                0x72 => (3, 14, &[4, 2, 2, 2, 2, 2, 4], false),
                                0x91 => (3, 4, &[4, 4], false),
                                0x99 => (2, 3, &[3, 3], true),
                                0x9f => (3, 16, &[4, 2, 2, 2, 2, 2, 2, 4], false),
                                0xd6 => (2, 5, &[3, 2, 3], false),
                                0xe8 => (3, 7, &[4, 1, 1, 1, 4], false),
                                _ => unreachable!(),
                            };
                            *degree == expected_degree
                                && control_points.len() == expected_controls
                                && knots.len() == expected_controls + expected_degree as usize + 1
                                && knots.iter().all(|knot| knot.is_finite())
                                && knots_nondecreasing(knots)
                                && knots[..=expected_degree as usize]
                                    .iter()
                                    .all(|knot| *knot == knots[0])
                                && knots[expected_controls..]
                                    .iter()
                                    .all(|knot| *knot == knots[expected_controls])
                                && knots[expected_degree as usize] < knots[expected_controls]
                                && knots
                                    .chunk_by(|left, right| left == right)
                                    .map(<[f64]>::len)
                                    .eq(expected_multiplicities.iter().copied())
                                && control_points
                                    .iter()
                                    .all(|point| point.u.is_finite() && point.v.is_finite())
                                && weights.as_ref().is_some_and(|weights| {
                                    rational
                                        && weights.len() == expected_controls
                                        && weights
                                            .iter()
                                            .all(|weight| weight.is_finite() && *weight > 0.0)
                                }) == rational
                        }
                        ([0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8], _) => false,
                        ([0x21, _], None) => true,
                        _ => false,
                    };
                    let expected_ordinal = u32::try_from(support_index)
                        .ok()
                        .and_then(|index| index.checked_add(1))
                        .and_then(|offset| run.carrier_record_ordinal.checked_add(offset));
                    support.tag[0] == 0x21
                        && zero_entity_record(records, support.record_ordinal).is_some_and(
                            |record| {
                                record.byte_offset == support.byte_offset
                                    && record.tag == support.tag
                            },
                        )
                        && support.byte_offset > run.carrier_byte_offset
                        && Some(support.record_ordinal) == expected_ordinal
                        && (support_index == 0
                            || run.supports[support_index - 1].byte_offset < support.byte_offset)
                        && endpoints_valid
                        && pcurve_valid
                        && model_curve_valid
                        && model_curve_construction_valid
                        && model_parameters_valid
                        && support.model_midpoint.is_some() == has_pcurve
                        && model_midpoint_valid
                        && model_endpoints_valid
                });
        if run.id != format!("catia:zero-entity:support-run#{index}")
            || !supports_valid
            || !face_roster_valid
            || !loop_roster_valid
            || !face_valid
            || run.carrier_record_ordinal == 0
            || zero_entity_record(records, run.carrier_record_ordinal)
                .is_none_or(|record| record.byte_offset != run.carrier_byte_offset)
            || index > 0
                && (runs[index - 1].carrier_byte_offset >= run.carrier_byte_offset
                    || runs[index - 1].carrier_record_ordinal >= run.carrier_record_ordinal)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "zero-entity support run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    Ok(())
}

fn validate_zero_entity_model_curve_construction(
    carrier_tag: Option<[u8; 2]>,
    model_curve: Option<&cadmpeg_ir::geometry::CurveGeometry>,
    construction: Option<&cadmpeg_ir::geometry::ProceduralCurveDefinition>,
) -> bool {
    let finite_vector = |vector: &cadmpeg_ir::math::Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
            && vector.x.hypot(vector.y).hypot(vector.z) > 0.0
    };
    let norm = |vector: &cadmpeg_ir::math::Vector3| vector.x.hypot(vector.y).hypot(vector.z);
    let normalized_dot = |left: &cadmpeg_ir::math::Vector3, right: &cadmpeg_ir::math::Vector3| {
        (left.x * right.x + left.y * right.y + left.z * right.z) / (norm(left) * norm(right))
    };
    match (carrier_tag, model_curve, construction) {
        (
            Some([0x29, 0xb8]),
            None,
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
                angle_range,
                center,
                major,
                minor,
                pitch,
                apex_factor,
                axis,
            }),
        ) => {
            angle_range.iter().copied().all(f64::is_finite)
                && angle_range[0] < angle_range[1]
                && [center.x, center.y, center.z]
                    .into_iter()
                    .all(f64::is_finite)
                && finite_vector(major)
                && finite_vector(minor)
                && [pitch.x, pitch.y, pitch.z].into_iter().all(f64::is_finite)
                && apex_factor.is_finite()
                && finite_vector(axis)
                && (norm(axis) - 1.0).abs() <= 1e-9
                && (norm(major) - norm(minor)).abs() <= 1e-9 * norm(major).max(norm(minor))
                && normalized_dot(major, minor).abs() <= 1e-9
                && normalized_dot(major, axis).abs() <= 1e-9
                && normalized_dot(minor, axis).abs() <= 1e-9
                && (pitch.x == 0.0 && pitch.y == 0.0 && pitch.z == 0.0
                    || normalized_dot(pitch, axis).abs() >= 1.0 - 1e-9)
                && {
                    let handed_minor = cadmpeg_ir::math::Vector3::new(
                        axis.y * major.z - axis.z * major.y,
                        axis.z * major.x - axis.x * major.z,
                        axis.x * major.y - axis.y * major.x,
                    );
                    normalized_dot(&handed_minor, minor) >= 1.0 - 1e-9
                }
        }
        (_, Some(_), None)
        | (Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8] | [0x34, 0xc8 | 0x5e]), None, None) => {
            true
        }
        _ => false,
    }
}

fn validate_zero_entity_model_curve(
    carrier_tag: Option<[u8; 2]>,
    curve: Option<&cadmpeg_ir::geometry::CurveGeometry>,
) -> bool {
    use cadmpeg_ir::geometry::CurveGeometry;

    let finite_point = |point: &cadmpeg_ir::math::Point3| {
        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
    };
    let finite_vector = |vector: &cadmpeg_ir::math::Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
            && vector.x.hypot(vector.y).hypot(vector.z) > 0.0
    };
    match (carrier_tag, curve) {
        (Some([0x27, 0x6a] | [0x34, 0xc8 | 0x5e]), Some(CurveGeometry::Nurbs(curve))) => {
            let Ok(degree) = usize::try_from(curve.degree) else {
                return false;
            };
            curve.control_points.len() > degree
                && curve.knots.len() == curve.control_points.len() + degree + 1
                && curve.knots.iter().all(|knot| knot.is_finite())
                && knots_nondecreasing(&curve.knots)
                && curve.control_points.iter().all(finite_point)
                && curve.weights.as_ref().is_none_or(|weights| {
                    weights.len() == curve.control_points.len()
                        && weights
                            .iter()
                            .all(|weight| weight.is_finite() && *weight > 0.0)
                })
                && !curve.periodic
        }
        (Some([0x28, 0x8a] | [0x29, 0xb8]), Some(CurveGeometry::Line { origin, direction })) => {
            finite_point(origin) && finite_vector(direction)
        }
        (
            Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8]),
            Some(CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            }),
        ) => {
            finite_point(center)
                && finite_vector(axis)
                && finite_vector(ref_direction)
                && radius.is_finite()
                && *radius > 0.0
        }
        (Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8] | [0x34, 0xc8 | 0x5e]), None) => true,
        _ => false,
    }
}

fn validate_zero_entity_endpoint_pair_candidates(
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
    runs: &[CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let expected = zero_entity_endpoint_pair_candidates(derived_zero_entity_endpoint_pairs(runs));
    if endpoint_pairs != expected {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity endpoint-pair candidates disagree with their radial support occurrences"
                .to_string(),
        ));
    }
    Ok(())
}

fn derived_zero_entity_endpoint_pairs(
    runs: &[CatiaZeroEntitySupportRun],
) -> Vec<crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate> {
    let mut occurrences = Vec::new();
    for run in runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let midpoints = run
            .supports
            .iter()
            .filter_map(|support| Some((support.record_ordinal, support.model_midpoint?)))
            .collect::<std::collections::HashMap<_, _>>();
        for loop_record in &face.loops {
            for (support_record_ordinal, model_endpoints) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .zip(loop_record.oriented_model_endpoints.iter().copied())
            {
                let Some(model_midpoint) = midpoints.get(&support_record_ordinal).copied() else {
                    continue;
                };
                occurrences.push(
                    crate::families::zero_entity::topology::ZeroEntityOrientedOccurrence {
                        face_record_ordinal: face.record_ordinal,
                        support_record_ordinal,
                        model_endpoints,
                        model_midpoint,
                    },
                );
            }
        }
    }
    crate::families::zero_entity::topology::endpoint_pair_candidates(&occurrences)
}

fn validate_zero_entity_endpoint_locus_candidates(
    endpoint_loci: &[CatiaZeroEntityEndpointLocusCandidate],
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
    runs: &[CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let derived_pairs = derived_zero_entity_endpoint_pairs(runs);
    let expected = zero_entity_endpoint_locus_candidates(
        crate::families::zero_entity::topology::endpoint_locus_candidates(&derived_pairs),
        endpoint_pairs,
    );
    if endpoint_loci != expected {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity endpoint-locus candidates disagree with their endpoint-pair endpoints"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_zero_entity_records(
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let valid = records.iter().enumerate().all(|(index, record)| {
        let ordinal = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1));
        Some(record.record_ordinal) == ordinal
            && record.id == format!("catia:zero-entity:record#{}", record.record_ordinal)
            && record.logical_end > record.byte_offset
            && (index == 0 || records[index - 1].logical_end <= record.byte_offset)
    });
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity record namespace is structurally invalid".to_string(),
        ))
    }
}

fn validate_zero_entity_ownership_roots(
    roots: &[CatiaZeroEntityOwnershipRoot],
    support_runs: &[CatiaZeroEntitySupportRun],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let bound_face_count = support_runs.iter().filter(|run| run.face.is_some()).count();
    let valid = roots.iter().enumerate().all(|(index, root)| {
        root.id == format!("catia:zero-entity:ownership-root#{index}")
            && root.face_slots.len() == bound_face_count
            && root
                .face_slots
                .iter()
                .copied()
                .eq((1..=u32::try_from(bound_face_count).unwrap_or(0)).rev())
            && [
                (
                    root.face_roster_record_ordinal,
                    root.face_roster_byte_offset,
                    [0x61, 0x42],
                ),
                (
                    root.shell_record_ordinal,
                    root.shell_byte_offset,
                    [0x60, 0x06],
                ),
                (
                    root.body_record_ordinal,
                    root.body_byte_offset,
                    [0x65, 0x08],
                ),
            ]
            .into_iter()
            .all(|(ordinal, byte_offset, tag)| {
                zero_entity_record(records, ordinal)
                    .is_some_and(|record| record.byte_offset == byte_offset && record.tag == tag)
            })
            && root.shell_record_ordinal == root.face_roster_record_ordinal.saturating_add(1)
            && root.body_record_ordinal == root.shell_record_ordinal.saturating_add(1)
            && (index == 0
                || roots[index - 1].face_roster_byte_offset < root.face_roster_byte_offset)
    });
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity ownership root is structurally invalid".to_string(),
        ))
    }
}

fn validate_zero_entity_topology_records(
    edge_strides: &[CatiaZeroEntityEdgeStride],
    oriented_use_pairs: &[CatiaZeroEntityOrientedUsePair],
    vertex_incidences: &[CatiaZeroEntityVertexIncidence],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let edge_strides_valid = edge_strides.iter().enumerate().all(|(index, record)| {
        record.id == format!("catia:zero-entity:edge-stride#{index}")
            && record.record_ordinal != 0
            && !record.allocations.contains(&0)
            && record.allocations[0].checked_sub(1) == Some(record.allocations[3])
            && record.allocations[0].checked_sub(2) == Some(record.allocations[4])
            && record.topology_refs
                == [
                    record.allocations[0],
                    record.allocations[3],
                    record.allocations[4],
                ]
            && record.surface_support_refs == [record.allocations[1], record.allocations[2]]
            && zero_entity_record(records, record.record_ordinal).is_some_and(|source| {
                source.byte_offset == record.byte_offset && source.tag == [0x5e, 0x1a]
            })
            && (index == 0
                || edge_strides[index - 1].byte_offset < record.byte_offset
                    && edge_strides[index - 1].record_ordinal < record.record_ordinal)
    });
    let pairs_valid = oriented_use_pairs.iter().enumerate().all(|(index, pair)| {
        pair.id == format!("catia:zero-entity:oriented-use-pair#{index}")
            && pair.header_record_ordinal != 0
            && zero_entity_record(records, pair.header_record_ordinal).is_some_and(|source| {
                source.byte_offset == pair.header_byte_offset && source.tag == [0x25, 0x69]
            })
            && (index == 0
                || oriented_use_pairs[index - 1].header_byte_offset < pair.header_byte_offset
                    && oriented_use_pairs[index - 1].header_record_ordinal
                        < pair.header_record_ordinal)
            && pair.uses.iter().enumerate().all(|(use_index, use_)| {
                let side = use_index as u32 + 1;
                use_.side == side
                    && !use_.allocations.contains(&0)
                    && zero_entity_record(records, use_.record_ordinal).is_some_and(|source| {
                        source.byte_offset == use_.byte_offset && source.tag == [0x06, 0x38]
                    })
                    && use_.byte_offset > pair.header_byte_offset
                    && (use_index == 0 || pair.uses[use_index - 1].byte_offset < use_.byte_offset)
                    && use_.record_ordinal == pair.header_record_ordinal.saturating_add(side)
                    && use_.allocations
                        == [
                            pair.base_columns[0].saturating_add(side),
                            pair.base_columns[1].saturating_add(side),
                        ]
            })
    });
    let incidences_valid = vertex_incidences.iter().enumerate().all(|(index, record)| {
        let expected_count = match record.tag {
            [0x05, 0x0b] => 2,
            [0x05, 0x10] => 3,
            [0x05, 0x15] => 4,
            _ => return false,
        };
        record.id == format!("catia:zero-entity:vertex-incidence#{index}")
            && record.record_ordinal != 0
            && !record.allocations.contains(&0)
            && zero_entity_record(records, record.record_ordinal).is_some_and(|source| {
                source.byte_offset == record.byte_offset && source.tag == record.tag
            })
            && record.allocations.len() == expected_count
            && record.vertex_record.as_deref()
                == zero_entity_vertex_owner(records, record.record_ordinal)
                    .map(|owner| owner.id.as_str())
            && (index == 0
                || vertex_incidences[index - 1].byte_offset < record.byte_offset
                    && vertex_incidences[index - 1].record_ordinal < record.record_ordinal)
    });
    if edge_strides_valid && pairs_valid && incidences_valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity topology records are structurally invalid".to_string(),
        ))
    }
}

fn validate_consolidated_owner_packets(
    packets: &[CatiaConsolidatedOwnerPacket],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, packet) in packets.iter().enumerate() {
        let valid_face_node = packet.face_node.is_none_or(|face_node| {
            face_node.byte_offset.checked_add(face_node.byte_len) == Some(packet.byte_offset)
                && face_node.target.checked_add(1) == packet.payload.final_reference()
        });
        let valid_payload = match &packet.payload {
            CatiaOwnerPacketPayload::FixedNine { numeric_tail, .. } => {
                numeric_tail.header[0] == 0x84
                    && matches!(numeric_tail.header[1], 0x41 | 0xc1)
                    && numeric_tail.header[4] == 0x0d
                    && numeric_tail.lower.iter().all(|value| value.is_finite())
                    && numeric_tail.upper.iter().all(|value| value.is_finite())
                    && numeric_tail.lower[0] < numeric_tail.upper[0]
                    && numeric_tail.lower[1] < numeric_tail.upper[1]
                    && numeric_tail.bounds.iter().all(|bounds| {
                        bounds[0].is_finite() && bounds[1].is_finite() && bounds[0] < bounds[1]
                    })
            }
            CatiaOwnerPacketPayload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty()
            }
        };
        if packet.id != format!("catia:consolidated:owner-packet#{:010}", packet.byte_offset)
            || !valid_payload
            || !valid_face_node
            || index > 0 && packets[index - 1].byte_offset >= packet.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated owner packet `{}` is structurally invalid",
                packet.id
            )));
        }
    }
    Ok(())
}

struct ConsolidatedSupportArenas<'a> {
    circles: &'a [CatiaConsolidatedCircle],
    cones: &'a [CatiaConsolidatedCone],
    cylinders: &'a [CatiaConsolidatedCylinder],
    embedded_cylinders: &'a [CatiaConsolidatedEmbeddedCylinder],
    groups: &'a [CatiaConsolidatedGroup],
    planes: &'a [CatiaConsolidatedPlaneCarrier],
    spheres: &'a [CatiaConsolidatedSphere],
    tori: &'a [CatiaConsolidatedTorus],
}

fn validate_consolidated_edge_runs(
    runs: &[CatiaConsolidatedEdgeRun],
    pcurves: &[CatiaConsolidatedPcurve],
    supports: &ConsolidatedSupportArenas<'_>,
    nodes: &[CatiaConsolidatedEdgeNode],
    vertex_identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let pcurves = pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let circles = supports
        .circles
        .iter()
        .map(|circle| (circle.id.as_str(), circle))
        .collect::<HashMap<_, _>>();
    let circle_offsets = circles
        .values()
        .map(|circle| circle.byte_offset)
        .collect::<HashSet<_>>();
    let cone_offsets = supports
        .cones
        .iter()
        .map(|cone| cone.byte_offset)
        .collect::<HashSet<_>>();
    let sphere_offsets = supports
        .spheres
        .iter()
        .map(|sphere| sphere.byte_offset)
        .collect::<HashSet<_>>();
    let torus_offsets = supports
        .tori
        .iter()
        .map(|torus| torus.byte_offset)
        .collect::<HashSet<_>>();
    let cylinder_offsets = supports
        .cylinders
        .iter()
        .map(|cylinder| cylinder.byte_offset)
        .collect::<HashSet<_>>();
    let group_offsets = supports
        .groups
        .iter()
        .map(|group| (group.id.as_str(), group.byte_offset))
        .collect::<HashMap<_, _>>();
    let embedded_cylinder_offsets = supports
        .embedded_cylinders
        .iter()
        .filter_map(|cylinder| {
            Some((
                cylinder.byte_offset,
                *group_offsets.get(cylinder.group.as_str())?,
            ))
        })
        .collect::<HashSet<_>>();
    let plane_offsets = supports
        .planes
        .iter()
        .filter(|plane| valid_consolidated_plane_geometry(&plane.payload))
        .map(|plane| plane.byte_offset)
        .collect::<HashSet<_>>();
    let mut run_nodes = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let token_limit = 1u32.checked_shl(u32::from(node.width) * 8);
        let uses_valid = node.uses.as_ref().is_none_or(|uses| {
            node.curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses.references == [[first, second], [second, node.curve_ref]]
                })
                && uses.senses == [0x88, 0x84]
                && node.parameter_selectors == [2, 1]
        });
        let definition_valid = node.definition.as_ref().is_none_or(|definition| {
            let token_limit = 1u32.checked_shl(u32::from(definition.width) * 8);
            let expected_data =
                crate::families::consolidated::records::consolidated_edge_definition_data(
                    definition.class,
                    &definition.payload,
                );
            node.uses.is_some()
                && matches!(definition.width, 1..=3)
                && matches!(definition.flag, 0x03 | 0x13 | 0x83)
                && matches!(definition.class, 0x23..=0x25)
                && token_limit.is_some_and(|limit| definition.header_token < limit)
                && !definition.payload.is_empty()
                && definition.byte_offset < node.byte_offset
                && definition.data == expected_data
        });
        let analytic_circle_valid = node.analytic_circle.as_ref().is_none_or(|binding| {
            let definition = node.definition.as_ref();
            let circle = circles.get(binding.circle.as_str());
            node.uses.is_some()
                && definition.is_some_and(|definition| {
                    definition.class == 0x23
                        && matches!(
                            definition.data,
                            Some(ConsolidatedEdgeDefinitionData::Scalar {
                                ref values,
                                ..
                            }) if values.len() == 8
                        )
                        && circle.is_some_and(|circle| {
                            binding.descriptor.byte_offset < circle.byte_offset
                                && circle.byte_offset < definition.byte_offset
                        })
                })
                && matches!(binding.descriptor.width, 1..=3)
                && matches!(binding.descriptor.flag, 0x03 | 0x13 | 0x83)
                && 1u32
                    .checked_shl(u32::from(binding.descriptor.width) * 8)
                    .is_some_and(|limit| binding.descriptor.header_token < limit)
                && !binding.descriptor.payload.is_empty()
        });
        let class25_descriptor_valid = node.class25_descriptor.as_ref().is_none_or(|descriptor| {
            node.uses.is_some()
                && node.definition.as_ref().is_some_and(|definition| {
                    definition.class == 0x25
                        && matches!(
                            definition.data,
                            Some(
                                ConsolidatedEdgeDefinitionData::Scalar25 { .. }
                                    | ConsolidatedEdgeDefinitionData::SegmentedScalar25 { .. }
                            )
                        )
                        && descriptor.byte_offset < definition.byte_offset
                })
                && matches!(descriptor.control, 0x02 | 0x0a)
                && matches!(descriptor.values.len(), 2 | 3)
                && descriptor.values.iter().all(|value| value.is_finite())
        });
        if node.id != format!("catia:consolidated:edge-node#{index}")
            || !matches!(node.width, 1..=3)
            || !matches!(node.flag, 0x03 | 0x13 | 0x83)
            || token_limit.is_some_and(|limit| node.header_token >= limit)
            || !uses_valid
            || !definition_valid
            || !analytic_circle_valid
            || !class25_descriptor_valid
            || index > 0 && nodes[index - 1].byte_offset >= node.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` is structurally invalid",
                node.id
            )));
        }
    }
    for (index, run) in runs.iter().enumerate() {
        let expected_id = format!("catia:consolidated:edge-run#{index}");
        let pcurve_offsets = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.byte_offset));
        let pcurve_ranges = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.range));
        let Some(node) = nodes_by_id.get(run.node.as_str()) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` references missing node `{}`",
                run.id, run.node
            )));
        };
        if !run_nodes.insert(run.node.as_str()) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` belongs to multiple runs",
                run.node
            )));
        }
        let loci_valid = run.shared_loci.as_ref().map_or_else(
            || run.endpoint_loci.is_none(),
            |loci| {
                loci.len() >= 2
                    && loci.iter().flatten().all(|value| value.is_finite())
                    && run.endpoint_loci
                        == loci
                            .first()
                            .copied()
                            .zip(loci.last().copied())
                            .map(|(first, last)| [first, last])
            },
        );
        let bindings_valid = run
            .support_bindings
            .iter()
            .flatten()
            .all(|binding| match binding {
                CatiaConsolidatedSupportBinding::Cylinder { byte_offset } => {
                    cylinder_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::EmbeddedCylinder {
                    byte_offset,
                    wrapper_byte_offset,
                } => embedded_cylinder_offsets.contains(&(*byte_offset, *wrapper_byte_offset)),
                CatiaConsolidatedSupportBinding::Circle { byte_offset } => {
                    circle_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Cone { byte_offset } => {
                    cone_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Sphere { byte_offset } => {
                    sphere_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Torus { byte_offset } => {
                    torus_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Plane { byte_offset } => {
                    plane_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::NurbsCarrier { offset, .. } => offset.is_finite(),
            });
        if run.id != expected_id
            || pcurve_offsets[0] != Some(run.byte_offset)
            || pcurve_offsets[1].is_none()
            || pcurve_offsets[0] >= pcurve_offsets[1]
            || pcurve_offsets[1].is_some_and(|offset| offset >= node.byte_offset)
            || pcurve_ranges != [Some(run.parameter_range), Some(run.parameter_range)]
            || run.parameter_range[0] >= run.parameter_range[1]
            || !run.parameter_range.iter().all(|value| value.is_finite())
            || !run.tolerance.is_finite()
            || run.tolerance < 0.0
            || node.uses.is_none()
            || !matches!(node.tail, 0x01 | 0x21)
            || !bindings_valid
            || !loci_valid
            || index > 0 && runs[index - 1].byte_offset >= run.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    let mut expected_nodes = nodes.to_vec();
    let expected_identities = consolidated_vertex_identities(&mut expected_nodes);
    if expected_nodes != nodes || expected_identities != vertex_identities {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "consolidated vertex identities disagree with edge incidence".to_string(),
        ));
    }
    Ok(())
}

fn validate_native_links(
    aliases: &[CatiaAliasRow],
    catalogs: &[CatiaCatalog],
    graphs: &[CatiaObjectGraph],
    segments: &[CatiaFinjplSegment],
    value_blocks: &[CatiaValueBlock],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for catalog in catalogs {
        let count_width = if catalog.declared_count <= 0x50 { 1 } else { 2 };
        let Some(mut expected_offset) = catalog.byte_offset.checked_add(6 + count_width) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an overflowing extent",
                catalog.id
            )));
        };
        let catalog_end = catalog.byte_offset.checked_add(catalog.byte_len);
        if catalog.id != format!("catia:outer:catalog#{:010}", catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an invalid source identity",
                catalog.id
            )));
        }
        for (index, entry) in catalog.entries.iter().enumerate() {
            let next_offset = catalog
                .entries
                .get(index + 1)
                .map(|next| next.byte_offset)
                .or(catalog_end);
            let encoded_len = next_offset.and_then(|next| next.checked_sub(entry.byte_offset));
            let value_len = u64::try_from(entry.value.len()).ok();
            if entry.byte_offset != expected_offset
                || entry.id != format!("catia:outer:catalog-entry#{:010}", entry.byte_offset)
                || !encoded_len.zip(value_len).is_some_and(|(encoded, value)| {
                    matches!(encoded.checked_sub(value), Some(1 | 5))
                })
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog entry `{}` has an invalid source extent",
                    entry.id
                )));
            }
            expected_offset = next_offset.expect("validated catalog end");
        }
        if Some(expected_offset) != catalog_end {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` entries do not cover its frame",
                catalog.id
            )));
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let parsed = container::finjpl_segments(&segment.data, 0, segment.data.len());
        let expected_id = format!("catia:outer:finjpl#{index}");
        if segment.id != expected_id
            || u64::try_from(segment.data.len()).ok() != Some(segment.byte_len)
            || segment.byte_offset.checked_add(segment.byte_len).is_none()
            || !matches!(parsed.as_slice(), [parsed]
                if parsed.range == (0..segment.data.len())
                    && parsed.type_word == segment.type_word
                    && finjpl_family(parsed.kind) == segment.family
                    && parsed.name == segment.name)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "FINJPL segment `{}` has an invalid retained view",
                segment.id
            )));
        }
    }
    if segments
        .windows(2)
        .any(|pair| pair[0].byte_offset.checked_add(pair[0].byte_len) != Some(pair[1].byte_offset))
    {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "CATIA FINJPL segment extents are not contiguous".to_string(),
        ));
    }
    for block in value_blocks {
        if block.id != format!("catia:outer:value-block#{:010}", block.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid source identity",
                block.id
            )));
        }
        let Some(catalog) = catalogs.iter().find(|catalog| catalog.id == block.catalog) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` references missing catalog `{}`",
                block.id, block.catalog
            )));
        };
        if block.byte_offset.checked_add(block.byte_len) != Some(catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` is not adjacent to catalog `{}`",
                block.id, block.catalog
            )));
        }
        let payload_len = u64::try_from(block.payload.len()).ok();
        if block.declared_len.checked_add(1) != Some(block.byte_len)
            || payload_len.and_then(|len| len.checked_add(6)) != Some(block.declared_len)
            || value_block::tokenize(&block.payload) != block.fields
            || value_schema_selections(&block.id, block.byte_offset, &block.fields, catalog)
                != block.schema_selections
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid derived view",
                block.id
            )));
        }
        let mut adjacent_graphs = graphs.iter().filter(|graph| {
            graph.byte_offset.checked_add(graph.byte_len) == Some(block.byte_offset)
        });
        let adjacent_graph = adjacent_graphs.next();
        if adjacent_graphs.next().is_some()
            || block.object_graph.as_deref() != adjacent_graph.map(|graph| graph.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid adjacent graph link",
                block.id
            )));
        }
    }
    for graph in graphs {
        let Some(graph_end) = graph.byte_offset.checked_add(graph.byte_len) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an overflowing extent",
                graph.id
            )));
        };
        let mut expected_record_offset = graph.byte_offset.checked_add(6);
        if graph.id != format!("catia:outer:object-graph#{:010}", graph.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid source identity",
                graph.id
            )));
        }
        if graph.finjpl_segment.as_deref()
            != containing_finjpl_segment(graph.byte_offset, graph.byte_len, segments)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid FINJPL segment link",
                graph.id
            )));
        }
        for record in &graph.records {
            if Some(record.byte_offset) != expected_record_offset
                || record.id != format!("catia:outer:object-record#{:010}", record.byte_offset)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid source extent",
                    record.id
                )));
            }
            expected_record_offset = record.byte_offset.checked_add(record.byte_len);
        }
        if expected_record_offset != Some(graph_end) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` records do not cover its frame",
                graph.id
            )));
        }
        let mut candidates = catalogs
            .iter()
            .filter(|catalog| catalog.byte_offset == graph_end)
            .chain(
                value_blocks
                    .iter()
                    .filter(|block| block.byte_offset == graph_end)
                    .filter_map(|block| {
                        catalogs.iter().find(|catalog| catalog.id == block.catalog)
                    }),
            );
        let catalog = candidates.next();
        if candidates.next().is_some()
            || graph.catalog_byte_offset != catalog.map(|catalog| catalog.byte_offset)
            || graph.catalog.as_deref() != catalog.map(|catalog| catalog.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid schema-catalog link",
                graph.id
            )));
        }
        for record in &graph.records {
            let expected_class = catalog.and_then(|catalog| {
                usize::try_from(record.class_ref?).ok().and_then(|ordinal| {
                    catalog
                        .entries
                        .get(ordinal)
                        .map(|entry| (entry.id.as_str(), entry.value.as_str()))
                })
            });
            if record.class_entry.as_deref() != expected_class.map(|(entry, _)| entry)
                || record.class_name.as_deref() != expected_class.map(|(_, value)| value)
                || record.repeated_reference_schema_selection
                    != repeated_reference_schema_selection(
                        record.repeated_reference_suffix.as_ref(),
                        catalog,
                    )
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid schema class",
                    record.id
                )));
            }
        }
    }
    let mut primary_graphs = graphs.iter().filter(|graph| {
        graph
            .outer_container
            .as_ref()
            .is_some_and(|container| container.class_name == "CATPrtCont")
    });
    let primary_graph = match (primary_graphs.next(), primary_graphs.next()) {
        (Some(graph), None) => Some(graph),
        _ => None,
    };
    for alias in aliases {
        if alias.id != format!("catia:outer:alias-row#{:010}", alias.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has an invalid source identity",
                alias.id
            )));
        }
        let expected = usize::from(alias.entity_record_ordinal)
            .checked_sub(1)
            .and_then(|index| {
                let graph = primary_graph?;
                let record = graph.records.get(index)?;
                Some((
                    graph.id.as_str(),
                    record.id.as_str(),
                    record.design_object.as_deref(),
                ))
            });
        let valid = expected.map_or_else(
            || {
                alias.object_graph.is_none()
                    && alias.object_record.is_none()
                    && alias.design_object.is_none()
            },
            |(graph, record, object)| {
                alias.object_graph.as_deref() == Some(graph)
                    && alias.object_record.as_deref() == Some(record)
                    && alias.design_object.as_deref() == object
            },
        );
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has invalid graph, record, or design-object links",
                alias.id
            )));
        }
        if let Some(group) = &alias.group {
            if group.target_slot != (u32::from(alias.f1[2]) | ((alias.f2 & 0x00ff_ffff) << 8))
                || !object_graph::is_alias_group_storage_prefix(&group.storage_prefix)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "alias row `{}` has invalid group storage",
                    alias.id
                )));
            }
        }
    }
    Ok(())
}

impl CatiaNative {
    /// Decode CATIA-native records directly from a synthesized record source.
    #[must_use]
    pub(crate) fn decode(bytes: &[u8]) -> Self {
        let consolidated_records = crate::wire::records::consolidated_records(bytes);
        Self::decode_with_records(bytes, &consolidated_records)
    }

    /// Load the typed CATIA namespace from generic native arenas.
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let mut catalogs: Vec<CatiaCatalog> = namespace.arena_as("catalogs")?;
        let entries: Vec<CatiaCatalogEntry> = namespace.arena_as("catalog_entries")?;
        let catalog_ids = catalogs
            .iter()
            .map(|catalog| catalog.id.as_str())
            .collect::<HashSet<_>>();
        if catalog_ids.len() != catalogs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA catalog identity".to_string(),
            ));
        }
        if let Some(entry) = entries
            .iter()
            .find(|entry| !catalog_ids.contains(entry.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog entry `{}` references missing catalog `{}`",
                entry.id, entry.parent
            )));
        }
        for catalog in &mut catalogs {
            catalog.entries = entries
                .iter()
                .filter(|entry| entry.parent == catalog.id)
                .cloned()
                .collect();
            catalog.entries.sort_by_key(|entry| entry.ordinal);
            if u32::try_from(catalog.entries.len())
                .ok()
                .and_then(|count| count.checked_add(1))
                != Some(catalog.declared_count)
                || catalog
                    .entries
                    .iter()
                    .enumerate()
                    .any(|(ordinal, entry)| usize::try_from(entry.ordinal).ok() != Some(ordinal))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog `{}` has an invalid entry sequence",
                    catalog.id
                )));
            }
        }
        let mut graphs: Vec<CatiaObjectGraph> = namespace.arena_as("object_graphs")?;
        let mut records: Vec<CatiaObjectRecord> = namespace.arena_as("object_graph_records")?;
        if namespace.version < CATIA_TYPED_OWNER_SLOT_VERSION {
            for record in &mut records {
                let roles = object_graph::head_roles(record.lead, &record.head);
                record.owner = roles
                    .owner_ref
                    .map(CatiaObjectOwner::Entity)
                    .or_else(|| roles.owner_literal.map(CatiaObjectOwner::UnassignedLiteral));
            }
        }
        let mut entity_records: Vec<CatiaEntityRecord> = namespace.arena_as("entity_records")?;
        if namespace.version < CATIA_NUMERIC_PAIR_VERSION {
            for entity in &mut entity_records {
                entity.numeric_pair = entity_table::parse_numeric_pair(&entity.value_payload);
            }
        }
        let row_chain_arena = if namespace
            .arenas
            .contains_key("schema_configuration_row_chains")
        {
            "schema_configuration_row_chains"
        } else {
            "configuration_row_chains"
        };
        let mut schema_configuration_row_chains: Vec<CatiaSchemaConfigurationRowChain> =
            namespace.arena_as(row_chain_arena)?;
        let mut reference_signature_cohorts: Vec<CatiaReferenceSignatureCohort> =
            namespace.arena_as("reference_signature_cohorts")?;
        if namespace.version < CATIA_REFERENCE_SIGNATURE_INCIDENCE_VERSION {
            for entity in &mut entity_records {
                entity.reference_signature = entity_table::parse_reference_signature(
                    &entity.value_payload,
                )
                .map(|production| CatiaReferenceSignature {
                    production,
                    first_entity: CatiaEntityReference::default(),
                    second_entity: CatiaEntityReference::default(),
                });
            }
        }
        if namespace.version < CATIA_SUFFIX_FRAMING_VERSION {
            for entity in &mut entity_records {
                entity.suffix_framing = entity_suffix_framing(&entity.record_suffix);
            }
        }
        if namespace.version < CATIA_ENTITY_SCHEMA_VALUE_INCIDENCE_VERSION
            || namespace.version < CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION
        {
            for entity in &mut entity_records {
                entity.relation_expression = relation_expression(
                    &entity.definition_schema_selections,
                    &entity.value_schema_selections,
                );
                entity.parameter_value = parameter_value(
                    entity.lead,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                );
                entity.constraint_range = resolved_constraint_range(
                    entity.lead,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                    &records,
                    &entity.object_graph,
                    entity.entity_id,
                );
                entity.definition_value = definition_value(
                    entity.lead,
                    &entity.definition_schema_selections,
                    &entity.value_fields,
                    entity.suffix_value.as_ref(),
                    entity.suffix_schema_selection.as_ref(),
                );
                entity.definition_chain_value = definition_chain_value(
                    entity.lead,
                    &entity.definition_schema_selections,
                    &entity.value_fields,
                    entity.suffix_value.as_ref(),
                    entity.suffix_schema_selection.as_ref(),
                );
            }
        }
        if namespace.version < CATIA_SUFFIX_EVALUATION_OFFSET_VERSION
            || namespace.version < CATIA_SUFFIX_TRAILER_8193_VERSION
        {
            for graph in &graphs {
                let catalog = graph.catalog.as_deref().and_then(|catalog_id| {
                    catalogs.iter().find(|catalog| catalog.id == catalog_id)
                });
                for entity in entity_records
                    .iter_mut()
                    .filter(|entity| entity.object_graph == graph.id)
                {
                    entity.suffix_value = entity_suffix_value(&entity.record_suffix);
                    entity.suffix_schema_selection =
                        entity_suffix_schema_selection(entity.suffix_value.as_ref(), catalog);
                    entity.parameter_value = parameter_value(
                        entity.lead,
                        &entity.value_schema_selections,
                        entity.suffix_value.as_ref(),
                    );
                    entity.constraint_range = resolved_constraint_range(
                        entity.lead,
                        &entity.value_schema_selections,
                        entity.suffix_value.as_ref(),
                        &records,
                        &entity.object_graph,
                        entity.entity_id,
                    );
                    entity.definition_value = definition_value(
                        entity.lead,
                        &entity.definition_schema_selections,
                        &entity.value_fields,
                        entity.suffix_value.as_ref(),
                        entity.suffix_schema_selection.as_ref(),
                    );
                    entity.definition_chain_value = definition_chain_value(
                        entity.lead,
                        &entity.definition_schema_selections,
                        &entity.value_fields,
                        entity.suffix_value.as_ref(),
                        entity.suffix_schema_selection.as_ref(),
                    );
                }
            }
        }
        if namespace.version < CATIA_RANGE_NOMINAL_VERSION {
            for entity in &mut entity_records {
                entity.range_interval = range_interval(
                    &entity.value_payload,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                    &records,
                    &entity.object_graph,
                    entity.entity_id,
                );
            }
        }
        let graph_ids = graphs
            .iter()
            .map(|graph| graph.id.as_str())
            .collect::<HashSet<_>>();
        if graph_ids.len() != graphs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object-graph identity".to_string(),
            ));
        }
        if let Some(record) = records
            .iter()
            .find(|record| !graph_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object record `{}` references missing graph `{}`",
                record.id, record.parent
            )));
        }
        let record_ids = records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<HashSet<_>>();
        let entity_record_ids = entity_records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<HashSet<_>>();
        if record_ids.len() != records.len() || entity_record_ids.len() != entity_records.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object or entity-record identity".to_string(),
            ));
        }
        if let Some(entity) = entity_records.iter().find(|entity| {
            !graph_ids.contains(entity.object_graph.as_str())
                || !record_ids.contains(entity.object_record.as_str())
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "entity record `{}` has a missing graph or object-record link",
                entity.id
            )));
        }
        let entity_classes_by_graph_identity = entity_class_index(&records);
        let (
            relation_expressions,
            relation_expression_entities,
            entities_by_graph_identity,
            terminal_nulls_by_graph,
            parameter_bindings,
        ) = semantic_entity_indices(&entity_records, &entity_classes_by_graph_identity);
        if namespace.version < CATIA_REFERENCE_SIGNATURE_ENTITY_VERSION {
            let entity_references = CatiaEntityReferenceIndex {
                entities: &entities_by_graph_identity,
                classes: &entity_classes_by_graph_identity,
                terminal_nulls: &terminal_nulls_by_graph,
            };
            for entity in &mut entity_records {
                if let Some(signature) = entity.reference_signature.take() {
                    entity.reference_signature = Some(reference_signature(
                        signature.production,
                        &entity.object_graph,
                        &entity_references,
                    ));
                }
            }
        }
        if namespace.version < CATIA_REFERENCE_SIGNATURE_FRAME_VERSION {
            for entity in &mut entity_records {
                let Some(signature) = &mut entity.reference_signature else {
                    continue;
                };
                let Some(production) =
                    entity_table::parse_reference_signature(&entity.value_payload)
                else {
                    continue;
                };
                signature.production = production;
            }
        }
        if namespace.version < CATIA_REFERENCE_SIGNATURE_PAIR_VERSION {
            let entity_references = CatiaEntityReferenceIndex {
                entities: &entities_by_graph_identity,
                classes: &entity_classes_by_graph_identity,
                terminal_nulls: &terminal_nulls_by_graph,
            };
            for entity in &mut entity_records {
                entity.reference_signature = entity_table::parse_reference_signature(
                    &entity.value_payload,
                )
                .map(|production| {
                    reference_signature(production, &entity.object_graph, &entity_references)
                });
            }
        }
        let expected_reference_signature_cohorts =
            derive_reference_signature_cohorts(&entity_records);
        if namespace.version < CATIA_DERIVED_NATIVE_ID_VERSION {
            reference_signature_cohorts = expected_reference_signature_cohorts;
        } else if reference_signature_cohorts != expected_reference_signature_cohorts {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "CATIA reference-signature cohorts are not canonical".to_string(),
            ));
        }
        if namespace.version < CATIA_TERMINAL_NULL_REFERENCE_VERSION {
            for graph in &graphs {
                let terminal_null = entity_records
                    .iter()
                    .filter(|entity| entity.object_graph == graph.id)
                    .map(|entity| entity.entity_id)
                    .max()
                    .and_then(|entity_id| entity_id.checked_add(1));
                for record in records
                    .iter_mut()
                    .filter(|record| record.parent == graph.id)
                {
                    for reference in &mut record.references {
                        reference.is_null = Some(reference.entity_id) == terminal_null;
                    }
                }
            }
        }
        if namespace.version < CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION
            || namespace.version < CATIA_TERMINAL_NULL_REFERENCE_VERSION
            || namespace.version < CATIA_FORMULA_OUTPUT_REFERENCE_VERSION
            || namespace.version < CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION
            || namespace.version < CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version < CATIA_RELATION_DEPENDENCY_OFFSET_VERSION
            || namespace.version < CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION
            || namespace.version < CATIA_FORMULA_REFERENCE_OFFSET_VERSION
            || namespace.version < CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.formula_relation = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        formula_relation(
                            &entity.definition_schema_selections,
                            entity.entity_id,
                            object,
                            &relation_expressions,
                            &CatiaEntityReferenceIndex {
                                entities: &entities_by_graph_identity,
                                classes: &entity_classes_by_graph_identity,
                                terminal_nulls: &terminal_nulls_by_graph,
                            },
                            &parameter_bindings,
                        )
                    });
            }
        }
        if namespace.version < CATIA_RELATION_PROGRAM_INSTANCE_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_CONTEXT_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_CLASS_VERSION
            || namespace.version < CATIA_RELATION_TYPED_REFERENCE_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_DEPENDENCY_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_INPUT_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_OUTPUT_VERSION
            || namespace.version < CATIA_RELATION_DEPENDENCY_OFFSET_VERSION
            || namespace.version < CATIA_RELATION_REFERENCE_OFFSET_VERSION
            || namespace.version < CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION
            || namespace.version < CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.relation_program_instance = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        relation_program_instance(
                            entity.entity_id,
                            object,
                            &CatiaEntityReferenceIndex {
                                entities: &entities_by_graph_identity,
                                classes: &entity_classes_by_graph_identity,
                                terminal_nulls: &terminal_nulls_by_graph,
                            },
                            &relation_expression_entities,
                            &parameter_bindings,
                        )
                    });
            }
        }
        if namespace.version < CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION
            || namespace.version < CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION
            || namespace.version < CATIA_CONSTRAINT_RANGE_STORAGE_INCIDENCE_VERSION
        {
            for entity in &mut entity_records {
                if let Some(range) = &mut entity.constraint_range {
                    (range.incoming_references, range.incoming_storage_references) =
                        entity_incidences(&records, &entity.object_graph, entity.entity_id);
                }
            }
        }
        if namespace.version < CATIA_CONFIGURATION_INCIDENCE_VERSION
            || namespace.version < CATIA_SCHEMA_CONFIGURATION_REFERENCE_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_CLASS_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version < CATIA_CONFIGURATION_PAYLOAD_OFFSET_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.schema_configuration_record = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        schema_configuration_record(
                            entity.entity_id,
                            object,
                            &entity.value_schema_selections,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                        )
                    });
                entity.schema_configuration_row_link = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        schema_configuration_row_link(
                            entity.entity_id,
                            object,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                        )
                    });
            }
        }
        let expected_schema_configuration_row_chains = derive_schema_configuration_row_chains(
            &entity_records,
            &entities_by_graph_identity,
            &entity_classes_by_graph_identity,
            &terminal_nulls_by_graph,
        );
        if namespace.version < CATIA_DERIVED_NATIVE_ID_VERSION
            || namespace.version < CATIA_SCHEMA_CONFIGURATION_NAMING_VERSION
        {
            schema_configuration_row_chains = expected_schema_configuration_row_chains;
        } else if schema_configuration_row_chains != expected_schema_configuration_row_chains {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "schema-configuration-row chains do not match their successor links".to_string(),
            ));
        }
        for graph in &mut graphs {
            graph.records = records
                .iter()
                .filter(|record| record.parent == graph.id)
                .cloned()
                .collect();
            graph.records.sort_by_key(|record| record.ordinal);
            let mut graph_entities = entity_records
                .iter()
                .filter(|entity| entity.object_graph == graph.id)
                .collect::<Vec<_>>();
            graph_entities.sort_by_key(|entity| entity.ordinal);
            let catalog = graph
                .catalog
                .as_ref()
                .and_then(|catalog_id| catalogs.iter().find(|catalog| catalog.id == *catalog_id));
            if !graph_entities.is_empty()
                && (graph_entities.len() != graph.records.len()
                    || graph_entities
                        .iter()
                        .enumerate()
                        .any(|(ordinal, entity)| entity.ordinal != ordinal as u64)
                    || graph_entities
                        .windows(2)
                        .any(|pair| pair[0].entity_id >= pair[1].entity_id)
                    || graph_entities
                        .iter()
                        .any(|entity| !valid_entity_record_shape(entity))
                    || graph_entities.iter().any(|entity| {
                        entity.reference_signature
                            != entity_table::parse_reference_signature(&entity.value_payload).map(
                                |production| {
                                    reference_signature(
                                        production,
                                        &graph.id,
                                        &CatiaEntityReferenceIndex {
                                            entities: &entities_by_graph_identity,
                                            classes: &entity_classes_by_graph_identity,
                                            terminal_nulls: &terminal_nulls_by_graph,
                                        },
                                    )
                                },
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_schema_selections
                            != definition_schema_selections(
                                &entity_table::parse_definition_schema_selectors(
                                    &entity.definition_prefix,
                                ),
                                catalog,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.value_schema_selections
                            != entity_value_schema_selections(
                                &entity.value_fields,
                                catalog,
                                &entity.value_packets,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.relation_expression
                            != relation_expression(
                                &entity.definition_schema_selections,
                                &entity.value_schema_selections,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_value != entity_suffix_value(&entity.record_suffix)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_framing != entity_suffix_framing(&entity.record_suffix)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_schema_selection
                            != entity_suffix_schema_selection(entity.suffix_value.as_ref(), catalog)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.parameter_value
                            != parameter_value(
                                entity.lead,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.range_interval
                            != range_interval(
                                &entity.value_payload,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                                &graph.records,
                                &graph.id,
                                entity.entity_id,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.constraint_range
                            != resolved_constraint_range(
                                entity.lead,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                                &graph.records,
                                &graph.id,
                                entity.entity_id,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_value
                            != definition_value(
                                entity.lead,
                                &entity.definition_schema_selections,
                                &entity.value_fields,
                                entity.suffix_value.as_ref(),
                                entity.suffix_schema_selection.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_chain_value
                            != definition_chain_value(
                                entity.lead,
                                &entity.definition_schema_selections,
                                &entity.value_fields,
                                entity.suffix_value.as_ref(),
                                entity.suffix_schema_selection.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.relation_program_instance
                            != object.and_then(|object| {
                                relation_program_instance(
                                    entity.entity_id,
                                    object,
                                    &CatiaEntityReferenceIndex {
                                        entities: &entities_by_graph_identity,
                                        classes: &entity_classes_by_graph_identity,
                                        terminal_nulls: &terminal_nulls_by_graph,
                                    },
                                    &relation_expression_entities,
                                    &parameter_bindings,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.schema_configuration_record
                            != object.and_then(|object| {
                                schema_configuration_record(
                                    entity.entity_id,
                                    object,
                                    &entity.value_schema_selections,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.schema_configuration_row_link
                            != object.and_then(|object| {
                                schema_configuration_row_link(
                                    entity.entity_id,
                                    object,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.formula_relation
                            != object.and_then(|object| {
                                formula_relation(
                                    &entity.definition_schema_selections,
                                    entity.entity_id,
                                    object,
                                    &relation_expressions,
                                    &CatiaEntityReferenceIndex {
                                        entities: &entities_by_graph_identity,
                                        classes: &entity_classes_by_graph_identity,
                                        terminal_nulls: &terminal_nulls_by_graph,
                                    },
                                    &parameter_bindings,
                                )
                            })
                    })
                    || graph_entities.windows(2).any(|pair| {
                        pair[0].byte_offset.checked_add(pair[0].byte_len)
                            != Some(pair[1].byte_offset)
                    })
                    || graph_entities.last().and_then(|entity| {
                        entity
                            .byte_offset
                            .checked_add(entity.byte_len)?
                            .checked_add(1)
                    }) != Some(graph.byte_offset))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has an invalid entity-table sequence",
                    graph.id
                )));
            }
            let record_ids = graph
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect::<Vec<_>>();
            let record_design_objects = graph
                .records
                .iter()
                .map(|record| record.design_object.clone())
                .collect::<Vec<_>>();
            let record_indices = graph
                .records
                .iter()
                .enumerate()
                .filter_map(|(index, record)| Some((record.entity_id?, index)))
                .collect::<HashMap<_, _>>();
            let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
            if record_indices.len()
                != graph
                    .records
                    .iter()
                    .filter(|record| record.entity_id.is_some())
                    .count()
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has duplicate entity identities",
                    graph.id
                )));
            }
            for (ordinal, record) in graph.records.iter().enumerate() {
                let expected_head_roles = object_graph::head_roles(record.lead, &record.head);
                let expected_owner = expected_head_roles
                    .owner_ref
                    .map(CatiaObjectOwner::Entity)
                    .or_else(|| {
                        expected_head_roles
                            .owner_literal
                            .map(CatiaObjectOwner::UnassignedLiteral)
                    });
                let expected_design_object = record
                    .owner_entity_id()
                    .map(|owner| design_object_id(graph.byte_offset, owner));
                let paired_entity = graph_entities.get(ordinal).copied();
                let expected_storage = resolved_storage_link(
                    record.storage_ref,
                    &record_ids,
                    &record_design_objects,
                    &record_indices,
                );
                if usize::try_from(record.ordinal).ok() != Some(ordinal)
                    || record.owner != expected_owner
                    || (record.class_ref, record.storage_ref)
                        != (
                            expected_head_roles.class_ref,
                            expected_head_roles.storage_ref,
                        )
                    || record.design_object != expected_design_object
                    || record.entity_record != paired_entity.map(|entity| entity.id.clone())
                    || record.entity_id != paired_entity.map(|entity| entity.entity_id)
                    || paired_entity.is_some_and(|entity| entity.object_record != record.id)
                    || (
                        record.storage_record.as_ref(),
                        record.storage_design_object.as_ref(),
                    ) != (expected_storage.0.as_ref(), expected_storage.1.as_ref())
                    || record.repeated_reference_suffix
                        != object_graph::repeated_reference_suffix(&record.payload)
                    || record.inline_body.as_ref().is_some_and(|body| {
                        (graph_entities.is_empty() && !object_graph::is_inline_body(body))
                            || body.first() != Some(&record.lead)
                            || !record.head.is_empty()
                            || record.owner.is_some()
                            || record.class_ref.is_some()
                            || record.storage_ref.is_some()
                            || record.payload.size != 0
                            || !record.payload.fields.is_empty()
                            || record.subtype != PayloadSubtype::Empty
                    })
                    || record.inline_body.is_none() && record.head.is_empty()
                    || record.references
                        != resolved_payload_references(
                            &record.payload,
                            &record_ids,
                            &record_design_objects,
                            &record_indices,
                            terminal_null_entity_id,
                        )
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "object graph `{}` has an invalid record sequence",
                        graph.id
                    )));
                }
            }
        }
        let mut value_blocks: Vec<CatiaValueBlock> = namespace.arena_as("value_blocks")?;
        let value_schema_selections: Vec<CatiaValueSchemaSelection> =
            namespace.arena_as("value_schema_selections")?;
        let value_block_ids = value_blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<HashSet<_>>();
        if value_block_ids.len() != value_blocks.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA value-block identity".to_string(),
            ));
        }
        let mut selections_by_block = HashMap::<String, Vec<CatiaValueSchemaSelection>>::new();
        for selection in value_schema_selections {
            if !value_block_ids.contains(&selection.parent) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "value selection `{}` references missing block `{}`",
                    selection.id, selection.parent
                )));
            }
            selections_by_block
                .entry(selection.parent.clone())
                .or_default()
                .push(selection);
        }
        for block in &mut value_blocks {
            block.schema_selections = selections_by_block.remove(&block.id).unwrap_or_default();
            block
                .schema_selections
                .sort_by_key(|selection| selection.offset);
        }
        let design_objects = design_objects(&graphs, &entity_records);
        if namespace.arenas.contains_key("design_objects") {
            let mut stored: Vec<CatiaDesignObject> = namespace.arena_as("design_objects")?;
            if namespace.version < CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION {
                let derived_by_id = design_objects
                    .iter()
                    .map(|object| (object.id.as_str(), object))
                    .collect::<HashMap<_, _>>();
                for object in &mut stored {
                    if let Some(derived) = derived_by_id.get(object.id.as_str()) {
                        object
                            .definition_chain_values
                            .clone_from(&derived.definition_chain_values);
                    }
                }
            }
            if namespace.version < CATIA_PARALLEL_REFERENCE_COLUMN_INCIDENCE_VERSION {
                let derived_by_id = design_objects
                    .iter()
                    .map(|object| (object.id.as_str(), object))
                    .collect::<HashMap<_, _>>();
                for object in &mut stored {
                    if let Some(derived) = derived_by_id.get(object.id.as_str()) {
                        object
                            .parallel_reference_table
                            .clone_from(&derived.parallel_reference_table);
                    }
                }
            }
            let stored_by_id = stored
                .iter()
                .map(|object| (object.id.as_str(), object))
                .collect::<HashMap<_, _>>();
            if stored_by_id.len() != stored.len()
                || stored.len() != design_objects.len()
                || design_objects
                    .iter()
                    .any(|object| stored_by_id.get(object.id.as_str()).copied() != Some(object))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    "stored CATIA design objects disagree with their object graph".to_string(),
                ));
            }
        }
        let mut finjpl_segments: Vec<CatiaFinjplSegment> =
            if namespace.arenas.contains_key("finjpl_segments") {
                namespace.arena_as("finjpl_segments")?
            } else {
                Vec::new()
            };
        finjpl_segments.sort_by_key(|segment| segment.byte_offset);
        if namespace.version < CATIA_OBJECT_GRAPH_SEGMENT_VERSION {
            for graph in &mut graphs {
                graph.finjpl_segment =
                    containing_finjpl_segment(graph.byte_offset, graph.byte_len, &finjpl_segments)
                        .map(str::to_owned);
            }
        }
        let mut external_references: Vec<CatiaExternalReference> =
            if namespace.arenas.contains_key("external_references") {
                namespace.arena_as("external_references")?
            } else {
                Vec::new()
            };
        external_references.sort_by_key(|reference| reference.byte_offset);
        let expected_external_references = external_reference_views(&finjpl_segments);
        if external_references != expected_external_references {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA external references disagree with their project-flags segments"
                    .to_string(),
            ));
        }
        let external_references = expected_external_references;
        let mut legacy_entity_runs: Vec<CatiaLegacyEntityRun> =
            if namespace.arenas.contains_key("legacy_entity_runs") {
                namespace.arena_as("legacy_entity_runs")?
            } else {
                Vec::new()
            };
        if namespace.version < CATIA_LEGACY_IDENTITY_LEAD_VERSION {
            for identity in legacy_entity_runs
                .iter_mut()
                .flat_map(|run| &mut run.identities)
            {
                identity.lead = 0x81;
            }
        }
        if namespace.version < CATIA_LEGACY_ROLE_SELECTOR_VERSION {
            for run in &mut legacy_entity_runs {
                for field in &mut run.text_fields {
                    if let Some(role) = &mut field.role {
                        role.entity_id = field.entity_id;
                        run.role_selectors.push(role.clone());
                    }
                }
                run.role_selectors.sort_by_key(|role| role.byte_offset);
                run.role_selectors.dedup_by_key(|role| role.byte_offset);
            }
        }
        if namespace.version < CATIA_LEGACY_SCHEMA_IDENTIFIER_VERSION {
            for program in legacy_entity_runs
                .iter_mut()
                .filter_map(|run| run.schema_program.as_mut())
            {
                program.identifiers = legacy_schema_identifiers(program).ok_or_else(|| {
                    cadmpeg_ir::NativeConvertError::InvalidOwner(
                        "legacy schema-program offset exceeds the platform index range".to_string(),
                    )
                })?;
            }
        }
        if namespace.version < CATIA_LEGACY_SCHEMA_BOUNDARY_VERSION {
            for program in legacy_entity_runs
                .iter_mut()
                .filter_map(|run| run.schema_program.as_mut())
            {
                program.boundary = CatiaLegacySchemaProgramBoundary::VendorFooter;
            }
        }
        if namespace.version < CATIA_LEGACY_EVALUATED_VALUE_NAME_VERSION {
            for run in &mut legacy_entity_runs {
                for index in 0..run.scalar_values.len() {
                    let entity_id = run.scalar_values[index].entity_id;
                    let value_offset = run.scalar_values[index].byte_offset;
                    let name = (run
                        .scalar_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.scalar_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.scalar_values[index].name = name.map(|(_, name)| name);
                }
                for index in 0..run.string_values.len() {
                    let entity_id = run.string_values[index].entity_id;
                    let value_offset = run.string_values[index].byte_offset;
                    let name = (run
                        .string_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.string_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.string_values[index].name = name.map(|(_, name)| name);
                }
                for index in 0..run.integer_values.len() {
                    let entity_id = run.integer_values[index].entity_id;
                    let value_offset = run.integer_values[index].byte_offset;
                    let name = (run
                        .integer_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.integer_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.integer_values[index].name = name.map(|(_, name)| name);
                }
            }
        }
        legacy_entity_runs.sort_by_key(|run| run.byte_offset);
        validate_legacy_entity_runs(
            &legacy_entity_runs,
            namespace.version >= CATIA_LEGACY_ROLE_FIELD_CODE_VERSION,
        )?;
        let mut preview_images: Vec<CatiaPreviewImage> =
            if namespace.arenas.contains_key("preview_images") {
                namespace.arena_as("preview_images")?
            } else {
                Vec::new()
            };
        preview_images.sort_by_key(|preview| preview.byte_offset);
        let expected_preview_images = preview_views(&finjpl_segments);
        if preview_images != expected_preview_images {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA previews disagree with their summary segments".to_string(),
            ));
        }
        let preview_images = expected_preview_images;
        let alias_rows: Vec<CatiaAliasRow> = namespace.arena_as("alias_rows")?;
        let mut consolidated_circles: Vec<CatiaConsolidatedCircle> =
            namespace.arena_as("consolidated_circles")?;
        consolidated_circles.sort_by_key(|circle| circle.byte_offset);
        validate_consolidated_circles(&consolidated_circles)?;
        let mut consolidated_class61_records: Vec<CatiaConsolidatedClass61Record> =
            namespace.arena_as("consolidated_class61_records")?;
        consolidated_class61_records.sort_by_key(|record| record.byte_offset);
        validate_consolidated_class61_records(&consolidated_class61_records)?;
        let mut consolidated_cone_faces: Vec<CatiaConsolidatedConeFace> =
            namespace.arena_as("consolidated_cone_faces")?;
        consolidated_cone_faces.sort_by_key(|face| face.byte_offset);
        let mut consolidated_cones: Vec<CatiaConsolidatedCone> =
            namespace.arena_as("consolidated_cones")?;
        consolidated_cones.sort_by_key(|cone| cone.byte_offset);
        validate_consolidated_cones(&consolidated_cones)?;
        let mut consolidated_cylinders: Vec<CatiaConsolidatedCylinder> =
            namespace.arena_as("consolidated_cylinders")?;
        consolidated_cylinders.sort_by_key(|cylinder| cylinder.byte_offset);
        validate_consolidated_cylinders(&consolidated_cylinders)?;
        let mut consolidated_groups: Vec<CatiaConsolidatedGroup> =
            namespace.arena_as("consolidated_groups")?;
        consolidated_groups.sort_by_key(|group| group.byte_offset);
        validate_consolidated_groups(&consolidated_groups)?;
        let mut consolidated_embedded_cylinders: Vec<CatiaConsolidatedEmbeddedCylinder> =
            namespace.arena_as("consolidated_embedded_cylinders")?;
        consolidated_embedded_cylinders.sort_by_key(|cylinder| cylinder.byte_offset);
        validate_consolidated_embedded_cylinders(
            &consolidated_embedded_cylinders,
            &consolidated_groups,
        )?;
        let mut consolidated_line_profiles: Vec<CatiaConsolidatedLineProfile> =
            namespace.arena_as("consolidated_line_profiles")?;
        consolidated_line_profiles.sort_by_key(|line| line.byte_offset);
        validate_consolidated_line_profiles(&consolidated_line_profiles)?;
        let mut consolidated_owner_packets: Vec<CatiaConsolidatedOwnerPacket> =
            namespace.arena_as("consolidated_owner_packets")?;
        consolidated_owner_packets.sort_by_key(|packet| packet.byte_offset);
        validate_consolidated_owner_packets(&consolidated_owner_packets)?;
        let mut consolidated_parameter_points: Vec<CatiaConsolidatedParameterPoint> =
            namespace.arena_as("consolidated_parameter_points")?;
        consolidated_parameter_points.sort_by_key(|point| point.byte_offset);
        validate_consolidated_parameter_points(&consolidated_parameter_points)?;
        validate_consolidated_cone_faces(&consolidated_cone_faces, &consolidated_parameter_points)?;
        let mut consolidated_plane_carriers: Vec<CatiaConsolidatedPlaneCarrier> =
            namespace.arena_as("consolidated_plane_carriers")?;
        consolidated_plane_carriers.sort_by_key(|carrier| carrier.byte_offset);
        validate_consolidated_plane_carriers(&consolidated_plane_carriers)?;
        let mut consolidated_pcurves: Vec<CatiaConsolidatedPcurve> =
            namespace.arena_as("consolidated_pcurves")?;
        consolidated_pcurves.sort_by_key(|pcurve| pcurve.byte_offset);
        validate_consolidated_pcurves(&consolidated_pcurves)?;
        let mut consolidated_reference_lists: Vec<CatiaConsolidatedReferenceList> =
            namespace.arena_as("consolidated_reference_lists")?;
        consolidated_reference_lists.sort_by_key(|list| list.byte_offset);
        validate_consolidated_reference_lists(&consolidated_reference_lists)?;
        let mut consolidated_revolutions: Vec<CatiaConsolidatedRevolution> =
            namespace.arena_as("consolidated_revolutions")?;
        consolidated_revolutions.sort_by_key(|revolution| revolution.byte_offset);
        validate_consolidated_revolutions(&consolidated_revolutions, &consolidated_circles)?;
        let mut consolidated_spheres: Vec<CatiaConsolidatedSphere> =
            namespace.arena_as("consolidated_spheres")?;
        consolidated_spheres.sort_by_key(|sphere| sphere.byte_offset);
        validate_consolidated_spheres(&consolidated_spheres)?;
        let mut consolidated_tori: Vec<CatiaConsolidatedTorus> =
            namespace.arena_as("consolidated_tori")?;
        consolidated_tori.sort_by_key(|torus| torus.byte_offset);
        validate_consolidated_tori(&consolidated_tori)?;
        let mut consolidated_edge_runs: Vec<CatiaConsolidatedEdgeRun> =
            namespace.arena_as("consolidated_edge_runs")?;
        consolidated_edge_runs.sort_by_key(|run| run.byte_offset);
        let mut consolidated_edge_nodes: Vec<CatiaConsolidatedEdgeNode> =
            namespace.arena_as("consolidated_edge_nodes")?;
        consolidated_edge_nodes.sort_by_key(|node| node.byte_offset);
        let consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity> =
            namespace.arena_as("consolidated_vertex_identities")?;
        let mut zero_entity_edge_strides: Vec<CatiaZeroEntityEdgeStride> =
            namespace.arena_as("zero_entity_edge_strides")?;
        zero_entity_edge_strides.sort_by_key(|record| record.byte_offset);
        let mut zero_entity_oriented_use_pairs: Vec<CatiaZeroEntityOrientedUsePair> =
            namespace.arena_as("zero_entity_oriented_use_pairs")?;
        zero_entity_oriented_use_pairs.sort_by_key(|pair| pair.header_byte_offset);
        let zero_entity_ownership_roots: Vec<CatiaZeroEntityOwnershipRoot> =
            namespace.arena_as("zero_entity_ownership_roots")?;
        let zero_entity_endpoint_pair_candidates: Vec<CatiaZeroEntityEndpointPairCandidate> =
            namespace.arena_as("zero_entity_endpoint_pair_candidates")?;
        let mut zero_entity_records: Vec<CatiaZeroEntityRecord> =
            namespace.arena_as("zero_entity_records")?;
        zero_entity_records.sort_by_key(|record| record.record_ordinal);
        validate_zero_entity_records(&zero_entity_records)?;
        let mut zero_entity_support_runs: Vec<CatiaZeroEntitySupportRun> =
            namespace.arena_as("zero_entity_support_runs")?;
        zero_entity_support_runs.sort_by_key(|run| run.carrier_byte_offset);
        validate_zero_entity_support_runs(&zero_entity_support_runs, &zero_entity_records)?;
        validate_zero_entity_ownership_roots(
            &zero_entity_ownership_roots,
            &zero_entity_support_runs,
            &zero_entity_records,
        )?;
        let zero_entity_endpoint_locus_candidates: Vec<CatiaZeroEntityEndpointLocusCandidate> =
            namespace.arena_as("zero_entity_endpoint_locus_candidates")?;
        validate_zero_entity_endpoint_pair_candidates(
            &zero_entity_endpoint_pair_candidates,
            &zero_entity_support_runs,
        )?;
        validate_zero_entity_endpoint_locus_candidates(
            &zero_entity_endpoint_locus_candidates,
            &zero_entity_endpoint_pair_candidates,
            &zero_entity_support_runs,
        )?;
        let mut zero_entity_vertex_incidences: Vec<CatiaZeroEntityVertexIncidence> =
            namespace.arena_as("zero_entity_vertex_incidences")?;
        zero_entity_vertex_incidences.sort_by_key(|record| record.byte_offset);
        validate_zero_entity_topology_records(
            &zero_entity_edge_strides,
            &zero_entity_oriented_use_pairs,
            &zero_entity_vertex_incidences,
            &zero_entity_records,
        )?;
        validate_consolidated_edge_runs(
            &consolidated_edge_runs,
            &consolidated_pcurves,
            &ConsolidatedSupportArenas {
                circles: &consolidated_circles,
                cones: &consolidated_cones,
                cylinders: &consolidated_cylinders,
                embedded_cylinders: &consolidated_embedded_cylinders,
                groups: &consolidated_groups,
                planes: &consolidated_plane_carriers,
                spheres: &consolidated_spheres,
                tori: &consolidated_tori,
            },
            &consolidated_edge_nodes,
            &consolidated_vertex_identities,
        )?;
        validate_native_links(
            &alias_rows,
            &catalogs,
            &graphs,
            &finjpl_segments,
            &value_blocks,
        )?;
        Ok(Self {
            version: namespace.version,
            alias_rows,
            catalogs,
            consolidated_circles,
            consolidated_class61_records,
            consolidated_cone_faces,
            consolidated_cones,
            consolidated_cylinders,
            consolidated_embedded_cylinders,
            consolidated_edge_nodes,
            consolidated_edge_runs,
            consolidated_groups,
            consolidated_line_profiles,
            consolidated_owner_packets,
            consolidated_parameter_points,
            consolidated_plane_carriers,
            consolidated_pcurves,
            consolidated_reference_lists,
            consolidated_revolutions,
            consolidated_spheres,
            consolidated_tori,
            consolidated_vertex_identities,
            design_objects,
            entity_records,
            external_references,
            finjpl_segments,
            legacy_entity_runs,
            object_graphs: graphs,
            preview_images,
            reference_signature_cohorts,
            schema_configuration_row_chains,
            value_blocks,
            zero_entity_edge_strides,
            zero_entity_oriented_use_pairs,
            zero_entity_ownership_roots,
            zero_entity_endpoint_pair_candidates,
            zero_entity_records,
            zero_entity_support_runs,
            zero_entity_endpoint_locus_candidates,
            zero_entity_vertex_incidences,
        })
    }

    /// Store the typed CATIA namespace into generic native arenas.
    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        store_projection(&CatiaArenaProjection::from(self), namespace)
    }
}
