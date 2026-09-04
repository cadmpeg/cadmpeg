// SPDX-License-Identifier: Apache-2.0
//! Identity framing for the pre-`7C05` design stream.

use cadmpeg_core::decode::View;

use crate::container;

const CATALOG_OPEN: &[u8] = b"\xde\x04\xfe\xfe\x12CATCatalogManager";
const SCHEMA_PROGRAM_PREFIX: &[u8] = b"\xfe\xfe\xfe";
const SCHEMA_PROGRAM_FOOTER: &[u8] = b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA";
#[cfg(test)]
pub(crate) const SCHEMA_PROGRAM_OFFSET_FROM_CATALOG: usize =
    CATALOG_OPEN.len() + SCHEMA_PROGRAM_PREFIX.len();
const TEXT_OPEN: &[u8] = b"\xe8\x00\x12\x01";
const SCALAR_OPEN: &[u8] = b"\xfe\x85\x88\x82\xfe";
const NAMED_SCALAR_OPEN: &[u8] = b"\xfe\x84\x88\x82\xfe";
const STRING_OPEN: &[u8] = b"\xfe\x85\x93\x82\xfe";
const INTEGER_OPEN: &[u8] = b"\xfe\x85\x9d\x82\xfe";
const TYPE_OPEN: &[u8] = b"\xfe\x84\x92\x82";

/// Length production used by one legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyTextEncoding {
    /// Nonzero one-byte inclusive length followed by the text and `FE`.
    U8InclusiveLength,
    /// Zero selector, little-endian `u32` byte length, text, and `FE`.
    ZeroU32Length,
    /// Nonzero one-byte inclusive length followed by text and an `E3` paged-role tail.
    U8InclusiveLengthE3RoleTail,
}

/// Framing production used by one legacy role selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyRoleSelectorEncoding {
    /// `80` followed by a nonzero little-endian `u32`.
    FixedU32,
    /// Page byte `D1..E4` followed by one low byte.
    Paged,
}

/// Stored representation of one legacy schema role name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyRoleName {
    /// Inclusive-length UTF-8 role name.
    Literal(String),
    /// Unresolved one-byte schema selector.
    Selector(u8),
}

impl LegacyRoleName {
    fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Selector(_) => None,
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Literal(value) => 1 + value.len(),
            Self::Selector(_) => 1,
        }
    }
}

/// One length-framed schema role and its selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRoleSelector {
    /// Offset of the literal length or schema-selector byte.
    pub offset: usize,
    /// Stored identity whose interval contains the role.
    pub entity_id: u32,
    /// Stored literal or unresolved role name.
    pub name: LegacyRoleName,
    /// Selector framing production.
    pub encoding: LegacyRoleSelectorEncoding,
    /// Stored selector following the role name.
    pub selector: u32,
    /// Field code when an `E8 <field-code:u16le> 01` opener follows immediately.
    pub field_code: Option<u16>,
}

/// One complete UTF-8 text field in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyTextField {
    /// Offset of the `E8 00 12 01` field opener.
    pub offset: usize,
    /// Stored identity whose interval contains the field.
    pub entity_id: u32,
    /// Text framing production.
    pub encoding: LegacyTextEncoding,
    /// Immediately preceding length-framed role and selector.
    pub role: Option<LegacyRoleSelector>,
    /// Decoded UTF-8 value.
    pub value: String,
}

/// One schema field bounded by consecutive role selectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySchemaField {
    /// Offset of the `E8 <field-code:u16le> 01` opener.
    pub offset: usize,
    /// Stored identity whose interval contains the field.
    pub entity_id: u32,
    /// Role selector that binds this field.
    pub role_offset: usize,
    /// Following role selector that closes the payload.
    pub boundary_role_offset: usize,
    /// Stored schema field code.
    pub field_code: u16,
    /// Exact bytes after the opener and before the boundary role.
    pub payload: Vec<u8>,
}

/// One typed parameter clause in a legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRelationParameter {
    /// Expression-local parameter.
    pub parameter: String,
    /// Source value type.
    pub value_type: String,
}

/// Result of a complete legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyRelationResult {
    /// `VoidType` result with a named output parameter.
    Void {
        /// Output parameter for the void relation.
        output: LegacyRelationParameter,
    },
    /// Non-void result type with no output parameter.
    Typed(String),
}

/// Parsed roles in a complete legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRelationSignature {
    /// Ordered input parameters.
    pub inputs: Vec<LegacyRelationParameter>,
    /// Result type and optional void output.
    pub result: LegacyRelationResult,
}

impl LegacyRelationSignature {
    pub(crate) fn output(&self) -> Option<&LegacyRelationParameter> {
        match &self.result {
            LegacyRelationResult::Void { output } => Some(output),
            LegacyRelationResult::Typed(_) => None,
        }
    }

    pub(crate) fn result_type(&self) -> &str {
        match &self.result {
            LegacyRelationResult::Void { .. } => "VoidType",
            LegacyRelationResult::Typed(result_type) => result_type,
        }
    }
}

/// Paired expression and type-signature fields owned by one identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRelation {
    /// Stored owner identity.
    pub entity_id: u32,
    /// Selector carried by the expression field's `body` role.
    pub body_selector: Option<u32>,
    /// Selector carried by the type-signature field's `param` role.
    pub parameter_selector: Option<u32>,
    /// Parameter identity selected by exact self-`body` and target-`param` roles.
    pub parameter_entity_id: Option<u32>,
    /// Expression-field opener offset.
    pub expression_offset: usize,
    /// Exact expression or rule program.
    pub expression: String,
    /// Signature-field opener offset.
    pub signature_offset: usize,
    /// Exact stored type signature.
    pub type_signature: String,
    /// Parsed input, output, and result roles.
    pub signature: LegacyRelationSignature,
}

/// One complete `synchrone` relation-update field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRelationSynchronousState {
    /// Offset of the `synchrone` role-name length byte.
    pub role_offset: usize,
    /// Stored identity whose interval contains the field.
    pub entity_id: u32,
    /// Selector carried by the `synchrone` role.
    pub selector: u32,
    /// Whether the relation updates synchronously.
    pub synchronous: bool,
}

/// Value selected by one legacy type descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyTypeValue {
    /// Inclusive-length UTF-8 type name.
    Name(String),
    /// Compact selector identity.
    Selector(u32),
}

/// One complete legacy type descriptor in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyTypeDescriptor {
    /// Offset of the fixed descriptor prefix.
    pub offset: usize,
    /// Stored identity whose interval contains the descriptor.
    pub entity_id: u32,
    /// Stored literal name or unresolved selector.
    pub value: LegacyTypeValue,
}

/// Evaluation stored by a complete legacy scalar packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyScalarEvaluation {
    /// `E6` followed by finite binary64 bits.
    Value(u64),
    /// `E7` without a scalar payload.
    Unset,
}

/// Fixed prefix selecting one legacy scalar production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyScalarEncoding {
    /// `FE 84 88 82 FE`.
    Named84,
    /// `FE 85 88 82 FE`.
    Standalone85,
}

/// One complete typed scalar packet in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyScalarValue {
    /// Offset of the fixed packet prefix.
    pub offset: usize,
    /// Stored identity whose interval contains the packet.
    pub entity_id: u32,
    /// Fixed scalar-prefix production.
    pub encoding: LegacyScalarEncoding,
    /// Unique co-owned `name` text-field opener.
    pub name_offset: Option<usize>,
    /// Unique co-owned stored name.
    pub name: Option<String>,
    /// Stored evaluation.
    pub evaluation: LegacyScalarEvaluation,
}

/// One complete UTF-8 string-value packet in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyStringValue {
    /// Offset of the fixed packet prefix.
    pub offset: usize,
    /// Stored identity whose interval contains the packet.
    pub entity_id: u32,
    /// Unique co-owned `name` text-field opener.
    pub name_offset: Option<usize>,
    /// Unique co-owned stored name.
    pub name: Option<String>,
    /// Stored UTF-8 value.
    pub value: String,
}

/// Stored encoding of one legacy signed integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyIntegerEncoding {
    /// One byte stores values zero through 126 as `value + 0x81`.
    Inline,
    /// `80` introduces one signed little-endian 32-bit value.
    WideI32,
}

/// One complete signed-integer packet in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyIntegerValue {
    /// Offset of the fixed packet prefix.
    pub offset: usize,
    /// Stored identity whose interval contains the packet.
    pub entity_id: u32,
    /// Stored integer encoding.
    pub encoding: LegacyIntegerEncoding,
    /// Unique co-owned `name` text-field opener.
    pub name_offset: Option<usize>,
    /// Unique co-owned stored name.
    pub name: Option<String>,
    /// Stored signed value.
    pub value: i32,
}

/// One stored entity identity in a legacy identity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyEntityIdentity {
    /// Offset of the `EA` identity delimiter.
    pub offset: usize,
    /// Little-endian identity following the delimiter.
    pub entity_id: u32,
    /// Stored record lead following the identity.
    pub lead: u8,
}

/// One complete compact schema program following a legacy catalog opener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySchemaProgram {
    /// Offset of the first program byte after the fixed prefix.
    pub offset: usize,
    /// Offset of the production following the program.
    pub boundary_offset: usize,
    /// Production that closes the program.
    pub boundary: LegacySchemaProgramBoundary,
    /// Exact program bytes, including the terminal `FE`.
    pub bytes: Vec<u8>,
    /// Complete inclusive-length identifier packets in source order.
    pub identifiers: Vec<LegacySchemaIdentifier>,
}

/// Production that closes a compact legacy schema program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySchemaProgramBoundary {
    /// Fixed vendor footer preceded by the terminal `FE`.
    VendorFooter,
    /// Validated outer stream directory preceded by the terminal `FE`.
    StreamDirectory,
}

/// One complete inclusive-length identifier packet in a compact schema program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySchemaIdentifier {
    /// Offset of the inclusive-length byte.
    pub offset: usize,
    /// Stored identifier.
    pub value: String,
}

/// A monotonically identified legacy run terminated by its schema catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyEntityRun {
    /// Offset of the fixed catalog opening production.
    pub catalog_offset: usize,
    /// Complete compact schema program following the catalog opener.
    pub schema_program: Option<LegacySchemaProgram>,
    /// Stored identities in source order.
    pub identities: Vec<LegacyEntityIdentity>,
    /// Complete length-framed role selectors in identity-interval order.
    pub role_selectors: Vec<LegacyRoleSelector>,
    /// Complete schema text fields contained by the identity intervals.
    pub text_fields: Vec<LegacyTextField>,
    /// Complete role-bounded schema fields.
    pub schema_fields: Vec<LegacySchemaField>,
    /// Complete expression/signature pairs.
    pub relations: Vec<LegacyRelation>,
    /// Complete `synchrone` relation-update fields.
    pub synchronous_states: Vec<LegacyRelationSynchronousState>,
    /// Complete literal or selector type descriptors.
    pub type_descriptors: Vec<LegacyTypeDescriptor>,
    /// Complete typed scalar packets.
    pub scalar_values: Vec<LegacyScalarValue>,
    /// Complete UTF-8 string-value packets.
    pub string_values: Vec<LegacyStringValue>,
    /// Complete signed-integer packets.
    pub integer_values: Vec<LegacyIntegerValue>,
}

/// Parse complete legacy identity runs terminated by the fixed schema-catalog opener.
#[must_use]
pub fn parse_runs(data: &[u8]) -> Vec<LegacyEntityRun> {
    let directory_offset = container::outer_stream_directory_range(data).map(|range| range.start);
    parse_runs_with_directory_offset(data, directory_offset)
}

fn parse_runs_with_directory_offset(
    data: &[u8],
    directory_offset: Option<usize>,
) -> Vec<LegacyEntityRun> {
    memchr::memmem::find_iter(data, CATALOG_OPEN)
        .filter_map(|catalog_offset| parse_run_before(data, catalog_offset, directory_offset))
        .collect()
}

fn parse_run_before(
    data: &[u8],
    catalog_offset: usize,
    directory_offset: Option<usize>,
) -> Option<LegacyEntityRun> {
    let mut identities = data[..catalog_offset]
        .windows(6)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            if bytes[0] != 0xea || !matches!(bytes[5], 0x81 | 0x82 | 0xe5 | 0xfd) {
                return None;
            }
            let entity_id = View::u32_le_at(bytes, 1)?;
            (entity_id != 0).then_some(LegacyEntityIdentity {
                offset,
                entity_id,
                lead: bytes[5],
            })
        })
        .collect::<Vec<_>>();
    identities.last()?;
    let suffix_start = identities
        .windows(2)
        .rposition(|pair| pair[0].entity_id >= pair[1].entity_id)
        .map_or(0, |index| index + 1);
    identities.drain(..suffix_start);
    if identities.first()?.entity_id != 1 {
        return None;
    }
    let mut role_selectors = Vec::new();
    let mut text_fields = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let start = identity.offset + 6;
        let end = identities
            .get(index + 1)
            .map_or(catalog_offset, |next| next.offset);
        let interval_roles = parse_role_selectors(data, start, end, identity.entity_id);
        text_fields.extend(parse_text_fields(
            data,
            start,
            end,
            identity.entity_id,
            &interval_roles,
        ));
        role_selectors.extend(interval_roles);
    }
    let relations = parse_relations(&text_fields, &identities);
    let schema_fields = parse_schema_fields(data, &role_selectors, &text_fields);
    let synchronous_states =
        parse_synchronous_states(data, &role_selectors, &identities, catalog_offset);
    let type_descriptors = identities
        .iter()
        .enumerate()
        .flat_map(|(index, identity)| {
            let start = identity.offset + 6;
            let end = identities
                .get(index + 1)
                .map_or(catalog_offset, |next| next.offset);
            parse_type_descriptors(data, start, end, identity.entity_id)
        })
        .collect();
    let mut scalar_values = identities
        .iter()
        .enumerate()
        .flat_map(|(index, identity)| {
            let start = identity.offset + 6;
            let end = identities
                .get(index + 1)
                .map_or(catalog_offset, |next| next.offset);
            parse_scalar_values(data, start, end, identity.entity_id)
        })
        .collect::<Vec<_>>();
    bind_scalar_names(data, &role_selectors, &text_fields, &mut scalar_values);
    let mut string_values = identities
        .iter()
        .enumerate()
        .flat_map(|(index, identity)| {
            let start = identity.offset + 6;
            let end = identities
                .get(index + 1)
                .map_or(catalog_offset, |next| next.offset);
            parse_string_values(data, start, end, identity.entity_id)
        })
        .collect::<Vec<_>>();
    bind_string_names(data, &role_selectors, &text_fields, &mut string_values);
    let mut integer_values = identities
        .iter()
        .enumerate()
        .flat_map(|(index, identity)| {
            let start = identity.offset + 6;
            let end = identities
                .get(index + 1)
                .map_or(catalog_offset, |next| next.offset);
            parse_integer_values(data, start, end, identity.entity_id)
        })
        .collect::<Vec<_>>();
    bind_integer_names(data, &role_selectors, &text_fields, &mut integer_values);
    Some(LegacyEntityRun {
        catalog_offset,
        schema_program: parse_schema_program(data, catalog_offset, directory_offset),
        identities,
        role_selectors,
        text_fields,
        schema_fields,
        relations,
        synchronous_states,
        type_descriptors,
        scalar_values,
        string_values,
        integer_values,
    })
}

fn parse_schema_program(
    data: &[u8],
    catalog_offset: usize,
    directory_offset: Option<usize>,
) -> Option<LegacySchemaProgram> {
    let prefix_offset = catalog_offset.checked_add(CATALOG_OPEN.len())?;
    let offset = prefix_offset.checked_add(SCHEMA_PROGRAM_PREFIX.len())?;
    if data.get(prefix_offset..offset)? != SCHEMA_PROGRAM_PREFIX {
        return None;
    }
    let search_end = memchr::memmem::find(&data[offset..], CATALOG_OPEN)
        .and_then(|relative| offset.checked_add(relative))
        .unwrap_or(data.len());
    let footer_offset = memchr::memmem::find_iter(&data[offset..search_end], SCHEMA_PROGRAM_FOOTER)
        .find_map(|relative| {
            let footer_offset = offset.checked_add(relative)?;
            (footer_offset > offset && data.get(footer_offset - 1) == Some(&0xfe))
                .then_some(footer_offset)
        });
    let (boundary_offset, boundary) = if let Some(footer_offset) = footer_offset {
        (footer_offset, LegacySchemaProgramBoundary::VendorFooter)
    } else {
        let directory_offset = directory_offset?;
        if directory_offset <= offset
            || directory_offset > search_end
            || data.get(directory_offset - 1) != Some(&0xfe)
        {
            return None;
        }
        (
            directory_offset,
            LegacySchemaProgramBoundary::StreamDirectory,
        )
    };
    let bytes = data.get(offset..boundary_offset)?.to_vec();
    Some(LegacySchemaProgram {
        offset,
        boundary_offset,
        boundary,
        identifiers: parse_schema_identifiers(&bytes, offset),
        bytes,
    })
}

pub(crate) fn parse_schema_identifiers(
    bytes: &[u8],
    program_offset: usize,
) -> Vec<LegacySchemaIdentifier> {
    bytes
        .iter()
        .enumerate()
        .filter_map(|(relative, first)| {
            let value_len = usize::from(*first).checked_sub(1)?;
            if value_len == 0 {
                return None;
            }
            let value_offset = relative.checked_add(1)?;
            let end = value_offset.checked_add(value_len)?;
            let value = std::str::from_utf8(bytes.get(value_offset..end)?).ok()?;
            if !valid_role_name(value) || bytes.get(end).is_some_and(|following| *following < 0x81)
            {
                return None;
            }
            Some(LegacySchemaIdentifier {
                offset: program_offset.checked_add(relative)?,
                value: value.to_owned(),
            })
        })
        .collect()
}

fn parse_synchronous_states(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    identities: &[LegacyEntityIdentity],
    catalog_offset: usize,
) -> Vec<LegacyRelationSynchronousState> {
    roles
        .iter()
        .filter_map(|role| {
            let at = role.end_offset()?;
            let interval_end = identities
                .iter()
                .find(|identity| identity.offset > role.offset)
                .map_or(catalog_offset, |identity| identity.offset);
            let (state, end) = match &role.name {
                LegacyRoleName::Literal(name) if name == "synchrone" => {
                    let end = at.checked_add(6)?;
                    let [0xe8, 0x00, 0x1c, 0x01, state, 0xfe] = *data.get(at..end)? else {
                        return None;
                    };
                    (state, end)
                }
                LegacyRoleName::Selector(_) => {
                    let end = at.checked_add(5)?;
                    let [0xe8, 0x00, 0x1c, 0x01, state] = *data.get(at..end)? else {
                        return None;
                    };
                    if !roles
                        .iter()
                        .any(|next| next.entity_id == role.entity_id && next.offset == end)
                    {
                        return None;
                    }
                    (state, end)
                }
                LegacyRoleName::Literal(_) => return None,
            };
            if end > interval_end {
                return None;
            }
            let synchronous = match state {
                0x81 => false,
                0x82 => true,
                _ => return None,
            };
            Some(LegacyRelationSynchronousState {
                role_offset: role.offset,
                entity_id: role.entity_id,
                selector: role.selector,
                synchronous,
            })
        })
        .collect()
}

fn parse_type_descriptors(
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Vec<LegacyTypeDescriptor> {
    memchr::memmem::find_iter(&data[start..end], TYPE_OPEN)
        .filter_map(|relative| {
            let offset = start + relative;
            let payload = offset.checked_add(TYPE_OPEN.len())?;
            let first = *data.get(payload)?;
            let value = if (2..=0x7f).contains(&first) {
                let name_end = payload.checked_add(usize::from(first))?;
                if name_end >= end || data.get(name_end) != Some(&0x83) {
                    return None;
                }
                let name = text_value(data.get(payload + 1..name_end)?)?;
                valid_role_name(&name).then_some(LegacyTypeValue::Name(name))?
            } else if (0x81..=0xd0).contains(&first)
                && payload.checked_add(1)? < end
                && data.get(payload + 1) == Some(&0x83)
            {
                LegacyTypeValue::Selector(u32::from(first - 0x80))
            } else {
                return None;
            };
            Some(LegacyTypeDescriptor {
                offset,
                entity_id,
                value,
            })
        })
        .collect()
}

fn parse_scalar_values(
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Vec<LegacyScalarValue> {
    let mut values = [
        (NAMED_SCALAR_OPEN, LegacyScalarEncoding::Named84),
        (SCALAR_OPEN, LegacyScalarEncoding::Standalone85),
    ]
    .into_iter()
    .flat_map(|(opener, encoding)| {
        memchr::memmem::find_iter(&data[start..end], opener).filter_map(move |relative| {
            let offset = start + relative;
            if offset.checked_add(6)? > end {
                return None;
            }
            let opcode = *data.get(offset + opener.len())?;
            let evaluation = match opcode {
                0xe6 => {
                    if offset.checked_add(14)? > end {
                        return None;
                    }
                    let bits = View::u64_le_at(data, offset + 6)?;
                    f64::from_bits(bits)
                        .is_finite()
                        .then_some(LegacyScalarEvaluation::Value(bits))?
                }
                0xe7 => LegacyScalarEvaluation::Unset,
                _ => return None,
            };
            Some(LegacyScalarValue {
                offset,
                entity_id,
                encoding,
                name_offset: None,
                name: None,
                evaluation,
            })
        })
    })
    .collect::<Vec<_>>();
    values.sort_by_key(|value| value.offset);
    values
}

fn bind_scalar_names(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &[LegacyTextField],
    values: &mut [LegacyScalarValue],
) {
    let mut counts = std::collections::HashMap::new();
    for value in values.iter() {
        *counts.entry(value.entity_id).or_insert(0usize) += 1;
    }
    for value in values {
        if counts.get(&value.entity_id) != Some(&1) {
            continue;
        }
        if let Some(name) = unique_value_name(data, roles, fields, value.entity_id, value.offset) {
            value.name_offset = Some(name.offset);
            value.name = Some(name.value.clone());
        }
    }
}

fn parse_string_values(
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Vec<LegacyStringValue> {
    memchr::memmem::find_iter(&data[start..end], STRING_OPEN)
        .filter_map(|relative| {
            let offset = start + relative;
            let payload = offset.checked_add(STRING_OPEN.len())?;
            let inclusive_length = usize::from(*data.get(payload)?);
            if inclusive_length == 0 {
                return None;
            }
            let value_end = payload.checked_add(inclusive_length)?;
            if value_end > end {
                return None;
            }
            let value = text_value_allow_empty(data.get(payload + 1..value_end)?)?;
            Some(LegacyStringValue {
                offset,
                entity_id,
                name_offset: None,
                name: None,
                value,
            })
        })
        .collect()
}

fn bind_string_names(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &[LegacyTextField],
    values: &mut [LegacyStringValue],
) {
    let mut counts = std::collections::HashMap::new();
    for value in values.iter() {
        *counts.entry(value.entity_id).or_insert(0usize) += 1;
    }
    for value in values {
        if counts.get(&value.entity_id) != Some(&1) {
            continue;
        }
        if let Some(name) = unique_value_name(data, roles, fields, value.entity_id, value.offset) {
            value.name_offset = Some(name.offset);
            value.name = Some(name.value.clone());
        }
    }
}

fn parse_integer_values(
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Vec<LegacyIntegerValue> {
    memchr::memmem::find_iter(&data[start..end], INTEGER_OPEN)
        .filter_map(|relative| {
            let offset = start + relative;
            let payload = offset.checked_add(INTEGER_OPEN.len())?;
            let lead = *data.get(payload)?;
            let (encoding, value, value_end) = if lead == 0x80 {
                let value_end = payload.checked_add(5)?;
                if value_end > end {
                    return None;
                }
                (
                    LegacyIntegerEncoding::WideI32,
                    View::i32_le_at(data, payload + 1)?,
                    value_end,
                )
            } else {
                (
                    LegacyIntegerEncoding::Inline,
                    i32::from(lead.checked_sub(0x81)?),
                    payload + 1,
                )
            };
            (value_end <= end).then_some(LegacyIntegerValue {
                offset,
                entity_id,
                encoding,
                name_offset: None,
                name: None,
                value,
            })
        })
        .collect()
}

fn bind_integer_names(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &[LegacyTextField],
    values: &mut [LegacyIntegerValue],
) {
    let mut counts = std::collections::HashMap::new();
    for value in values.iter() {
        *counts.entry(value.entity_id).or_insert(0usize) += 1;
    }
    for value in values {
        if counts.get(&value.entity_id) != Some(&1) {
            continue;
        }
        if let Some(name) = unique_value_name(data, roles, fields, value.entity_id, value.offset) {
            value.name_offset = Some(name.offset);
            value.name = Some(name.value.clone());
        }
    }
}

fn unique_value_name<'a>(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &'a [LegacyTextField],
    entity_id: u32,
    value_offset: usize,
) -> Option<&'a LegacyTextField> {
    let mut names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("name"))
    });
    if let Some(name) = names.next() {
        return names.next().is_none().then_some(name);
    }
    unique_evaluated_value_name(data, roles, fields, entity_id, value_offset)
}

fn unique_evaluated_value_name<'a>(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &'a [LegacyTextField],
    entity_id: u32,
    value_offset: usize,
) -> Option<&'a LegacyTextField> {
    const EVALUATION_FIELD: &[u8] = b"\xe8\xc4\x17\x01\xfe\xfe";

    let mut evaluation_roles = roles.iter().filter(|role| {
        role.entity_id == entity_id
            && role.field_code == Some(0x17c4)
            && role
                .end_offset()
                .and_then(|offset| offset.checked_add(EVALUATION_FIELD.len()))
                == Some(value_offset)
            && role
                .end_offset()
                .and_then(|offset| data.get(offset..value_offset))
                == Some(EVALUATION_FIELD)
    });
    let evaluation_role = evaluation_roles.next()?;
    if evaluation_roles.next().is_some() {
        return None;
    }
    let mut names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field.offset < evaluation_role.offset
            && valid_role_name(&field.value)
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.field_code == Some(0x1200))
    });
    let name = names.next()?;
    names.next().is_none().then_some(name)
}

fn parse_relations(
    fields: &[LegacyTextField],
    identities: &[LegacyEntityIdentity],
) -> Vec<LegacyRelation> {
    let mut relations = Vec::new();
    let mut start = 0;
    while start < fields.len() {
        let entity_id = fields[start].entity_id;
        let end = fields[start..]
            .iter()
            .position(|field| field.entity_id != entity_id)
            .map_or(fields.len(), |relative| start + relative);
        let entity_fields = &fields[start..end];
        let mut expressions = entity_fields.iter().filter(|field| {
            field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("body"))
        });
        let expression = expressions.next();
        let duplicate_expression = expressions.next();
        let mut signatures = entity_fields.iter().filter(|field| {
            field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("param"))
        });
        let signature = signatures.next();
        let duplicate_signature = signatures.next();
        let role_bound_pair = match (
            expression,
            duplicate_expression,
            signature,
            duplicate_signature,
        ) {
            (Some(expression), None, Some(signature), None)
                if expression.offset < signature.offset =>
            {
                Some((expression, signature))
            }
            _ => None,
        };
        let selected_role_pair = match entity_fields {
            [prelude, expression, signature]
                if prelude.value.is_empty()
                    && prelude
                        .role
                        .as_ref()
                        .is_none_or(|role| matches!(&role.name, LegacyRoleName::Selector(_)))
                    && prelude.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && expression.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && signature.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && expression
                        .role
                        .as_ref()
                        .is_some_and(|role| matches!(&role.name, LegacyRoleName::Selector(_)))
                    && signature
                        .role
                        .as_ref()
                        .is_some_and(|role| matches!(&role.name, LegacyRoleName::Selector(_))) =>
            {
                Some((expression, signature))
            }
            _ => None,
        };
        let pair = role_bound_pair.or(selected_role_pair).or_else(|| {
            let [expression, signature] = entity_fields else {
                return None;
            };
            Some((expression, signature))
        });
        if let Some((expression, type_signature)) = pair {
            if let Some(signature) = parse_relation_signature(&type_signature.value) {
                let body_selector = relation_role_selector(expression, "body");
                let parameter_selector = relation_role_selector(type_signature, "param");
                relations.push(LegacyRelation {
                    entity_id,
                    body_selector,
                    parameter_selector,
                    parameter_entity_id: relation_parameter_entity(
                        entity_id,
                        body_selector,
                        parameter_selector,
                        identities,
                    ),
                    expression_offset: expression.offset,
                    expression: expression.value.clone(),
                    signature_offset: type_signature.offset,
                    type_signature: type_signature.value.clone(),
                    signature,
                });
            }
        }
        start = end;
    }
    relations
}

fn relation_role_selector(field: &LegacyTextField, role_name: &str) -> Option<u32> {
    field
        .role
        .as_ref()
        .filter(|role| role.name.literal() == Some(role_name))
        .map(|role| role.selector)
}

fn relation_parameter_entity(
    entity_id: u32,
    body_selector: Option<u32>,
    parameter_selector: Option<u32>,
    identities: &[LegacyEntityIdentity],
) -> Option<u32> {
    let parameter_selector = parameter_selector?;
    (body_selector == Some(entity_id)
        && identities
            .iter()
            .any(|identity| identity.entity_id == parameter_selector))
    .then_some(parameter_selector)
}

/// Parse a complete legacy relation type signature.
#[must_use]
pub fn parse_relation_signature(source: &str) -> Option<LegacyRelationSignature> {
    let source = source.strip_suffix('\n').unwrap_or(source);
    let (clauses, result_type) = source.rsplit_once(") : ")?;
    let clauses = clauses.strip_prefix('(')?;
    let result_type = result_type.trim();
    if result_type.is_empty() {
        return None;
    }
    let mut inputs = Vec::new();
    let mut output = None;
    let mut names = std::collections::HashSet::new();
    if !clauses.trim().is_empty() {
        for clause in clauses.split(',') {
            let (parameter, role_type) = clause.split_once(':')?;
            let parameter = parameter.trim();
            let role_type = role_type.trim();
            let (output_role, value_type) = if let Some(value_type) = role_type.strip_prefix("#In")
            {
                (false, value_type.trim())
            } else {
                (true, role_type.strip_prefix("#Out")?.trim())
            };
            if parameter.is_empty() || value_type.is_empty() || !names.insert(parameter) {
                return None;
            }
            let parameter = LegacyRelationParameter {
                parameter: parameter.to_string(),
                value_type: value_type.to_string(),
            };
            if output_role {
                if output.replace(parameter).is_some() {
                    return None;
                }
            } else {
                inputs.push(parameter);
            }
        }
    }
    let result = if result_type == "VoidType" {
        LegacyRelationResult::Void { output: output? }
    } else if output.is_none() {
        LegacyRelationResult::Typed(result_type.to_string())
    } else {
        return None;
    };
    Some(LegacyRelationSignature { inputs, result })
}

fn parse_text_fields(
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
    role_selectors: &[LegacyRoleSelector],
) -> Vec<LegacyTextField> {
    memchr::memmem::find_iter(&data[start..end], TEXT_OPEN)
        .filter_map(|relative| {
            let offset = start + relative;
            let payload = offset.checked_add(TEXT_OPEN.len())?;
            parse_text_field(data, payload, end).map(|(encoding, value)| LegacyTextField {
                offset,
                entity_id,
                encoding,
                role: role_selectors
                    .iter()
                    .find(|role| role.end_offset() == Some(offset))
                    .cloned(),
                value,
            })
        })
        .collect()
}

impl LegacyRoleSelector {
    fn end_offset(&self) -> Option<usize> {
        self.offset
            .checked_add(self.name.byte_len())?
            .checked_add(match self.encoding {
                LegacyRoleSelectorEncoding::FixedU32 => 5,
                LegacyRoleSelectorEncoding::Paged => 2,
            })
    }
}

fn parse_schema_fields(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    text_fields: &[LegacyTextField],
) -> Vec<LegacySchemaField> {
    roles
        .windows(2)
        .filter_map(|pair| {
            let [role, boundary] = pair else {
                return None;
            };
            if role.entity_id != boundary.entity_id {
                return None;
            }
            let offset = role.end_offset()?;
            let payload_offset = offset.checked_add(4)?;
            if payload_offset > boundary.offset
                || data.get(offset) != Some(&0xe8)
                || data.get(offset + 3) != Some(&0x01)
            {
                return None;
            }
            let boundary_binds_field = boundary.end_offset().is_some_and(|next_offset| {
                data.get(next_offset) == Some(&0xe8)
                    && next_offset.checked_add(3).and_then(|at| data.get(at)) == Some(&0x01)
            });
            let boundary_closes_text = text_fields.iter().any(|field| {
                field.offset == offset
                    && field.entity_id == role.entity_id
                    && field.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && field.role.as_ref().is_some_and(|bound| bound == role)
                    && payload_offset
                        .checked_add(1)
                        .and_then(|value_offset| value_offset.checked_add(field.value.len()))
                        == Some(boundary.offset)
            });
            if !boundary_binds_field && !boundary_closes_text {
                return None;
            }
            Some(LegacySchemaField {
                offset,
                entity_id: role.entity_id,
                role_offset: role.offset,
                boundary_role_offset: boundary.offset,
                field_code: View::u16_le_at(data, offset + 1)?,
                payload: data.get(payload_offset..boundary.offset)?.to_vec(),
            })
        })
        .collect()
}

fn parse_role_selectors(
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Vec<LegacyRoleSelector> {
    let mut roles = (start..end)
        .filter_map(|offset| {
            let inclusive_length = usize::from(*data.get(offset)?);
            if !(2..=u8::MAX as usize).contains(&inclusive_length) {
                return None;
            }
            let selector_offset = offset.checked_add(inclusive_length)?;
            if selector_offset >= end {
                return None;
            }
            let name = text_value(data.get(offset + 1..selector_offset)?)?;
            if !valid_role_name(&name) {
                return None;
            }
            let first = *data.get(selector_offset)?;
            let (encoding, selector) = if first == 0x80 {
                let selector_end = selector_offset.checked_add(5)?;
                if selector_end > end {
                    return None;
                }
                (
                    LegacyRoleSelectorEncoding::FixedU32,
                    View::u32_le_at(data, selector_offset + 1)?,
                )
            } else if (0xd1..=0xe4).contains(&first) {
                if selector_offset.checked_add(2)? > end {
                    return None;
                }
                (
                    LegacyRoleSelectorEncoding::Paged,
                    u32::from(first - 0xd1)
                        .checked_mul(256)?
                        .checked_add(u32::from(*data.get(selector_offset + 1)?))?
                        .checked_add(1)?,
                )
            } else {
                return None;
            };
            (selector != 0).then_some(LegacyRoleSelector {
                offset,
                entity_id,
                name: LegacyRoleName::Literal(name),
                encoding,
                selector,
                field_code: None,
            })
        })
        .collect::<Vec<_>>();
    roles.extend(
        memchr::memmem::find_iter(&data[start..end], TEXT_OPEN).filter_map(|relative| {
            let payload = start.checked_add(relative)?.checked_add(TEXT_OPEN.len())?;
            let value_length = usize::from(*data.get(payload)?).checked_sub(1)?;
            let role_offset = payload.checked_add(1)?.checked_add(value_length)?;
            let name_selector = *data.get(role_offset)?;
            let page_offset = role_offset.checked_add(1)?;
            let low_offset = role_offset.checked_add(2)?;
            if name_selector == 0 || data.get(page_offset) != Some(&0xe3) {
                return None;
            }
            // A declared one-byte text field owns its following FE terminator.
            // Do not reinterpret that same byte as an unresolved role selector
            // merely because E3 follows it (DI-25).
            if length_closed_text(data, payload + 1, value_length, end).is_some() {
                return None;
            }
            let selector_low = *data.get(low_offset)?;
            if role_offset.checked_add(3)? > end
                || text_value_allow_empty(data.get(payload.checked_add(1)?..role_offset)?).is_none()
            {
                return None;
            }
            Some(LegacyRoleSelector {
                offset: role_offset,
                entity_id,
                name: LegacyRoleName::Selector(name_selector),
                encoding: LegacyRoleSelectorEncoding::Paged,
                selector: u32::from(0xe3_u8 - 0xd1)
                    .checked_mul(256)?
                    .checked_add(u32::from(selector_low))?
                    .checked_add(1)?,
                field_code: None,
            })
        }),
    );
    let field_bound_roles = memchr::memchr_iter(0xe8, &data[start..end])
        .filter_map(|relative| {
            let field_offset = start.checked_add(relative)?;
            let field_header_end = field_offset.checked_add(4)?;
            if field_header_end > end || data.get(field_offset + 3) != Some(&0x01) {
                return None;
            }
            if roles
                .iter()
                .any(|role| role.end_offset() == Some(field_offset))
            {
                return None;
            }
            let fixed = field_offset
                .checked_sub(6)
                .filter(|offset| *offset >= start)
                .and_then(|role_offset| {
                    let name_selector = *data.get(role_offset)?;
                    (name_selector != 0 && data.get(role_offset + 1) == Some(&0x80))
                        .then_some(())?;
                    let selector = View::u32_le_at(data, role_offset + 2)?;
                    (selector != 0).then_some(LegacyRoleSelector {
                        offset: role_offset,
                        entity_id,
                        name: LegacyRoleName::Selector(name_selector),
                        encoding: LegacyRoleSelectorEncoding::FixedU32,
                        selector,
                        field_code: None,
                    })
                });
            let paged = field_offset
                .checked_sub(3)
                .filter(|offset| *offset >= start)
                .and_then(|role_offset| {
                    let name_selector = *data.get(role_offset)?;
                    let page = *data.get(role_offset + 1)?;
                    if name_selector == 0 || !(0xd1..=0xe4).contains(&page) {
                        return None;
                    }
                    let low = *data.get(role_offset + 2)?;
                    Some(LegacyRoleSelector {
                        offset: role_offset,
                        entity_id,
                        name: LegacyRoleName::Selector(name_selector),
                        encoding: LegacyRoleSelectorEncoding::Paged,
                        selector: u32::from(page - 0xd1)
                            .checked_mul(256)?
                            .checked_add(u32::from(low))?
                            .checked_add(1)?,
                        field_code: None,
                    })
                });
            // DI-24: fixed-width and paged selector layouts have no
            // precedence when both fit the same field boundary.
            match (fixed, paged) {
                (Some(_), Some(_)) => None,
                (Some(role), None) | (None, Some(role)) => Some(role),
                (None, None) => None,
            }
        })
        .collect::<Vec<_>>();
    roles.extend(field_bound_roles);
    roles.sort_by_key(|role| role.offset);
    roles.dedup_by_key(|role| role.offset);
    for role in &mut roles {
        role.field_code = role.end_offset().and_then(|offset| {
            (offset.checked_add(4)? <= end
                && data.get(offset) == Some(&0xe8)
                && data.get(offset + 3) == Some(&0x01))
            .then(|| View::u16_le_at(data, offset + 1))?
        });
    }
    roles
}

fn valid_role_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn parse_text_field(
    data: &[u8],
    payload: usize,
    end: usize,
) -> Option<(LegacyTextEncoding, String)> {
    let first = *data.get(payload)?;
    if first == 0 {
        if let Some(length) =
            View::u32_le_at(data, payload + 1).and_then(|n| usize::try_from(n).ok())
        {
            if let Some(value) = length_closed_text(data, payload + 5, length, end) {
                return Some((LegacyTextEncoding::ZeroU32Length, value));
            }
        }
    } else if let Some(length) = usize::from(first).checked_sub(1) {
        if let Some(value) = length_closed_text(data, payload + 1, length, end) {
            return Some((LegacyTextEncoding::U8InclusiveLength, value));
        }
        if let Some(value) = role_tailed_text(data, payload + 1, length, end) {
            return Some((LegacyTextEncoding::U8InclusiveLengthE3RoleTail, value));
        }
    }
    None
}

fn role_tailed_text(data: &[u8], start: usize, length: usize, end: usize) -> Option<String> {
    let value_end = start.checked_add(length)?;
    if value_end >= end {
        return None;
    }
    if value_end.checked_add(3)? <= end
        && data.get(value_end).is_some_and(|selector| *selector != 0)
        && data.get(value_end + 1) == Some(&0xe3)
    {
        return text_value_allow_empty(data.get(start..value_end)?);
    }
    let role_length = usize::from(*data.get(value_end)?);
    if role_length < 2 {
        return None;
    }
    let separator = value_end.checked_add(role_length)?;
    let tail_end = separator.checked_add(2)?;
    if tail_end > end
        || data.get(separator) != Some(&0xe3)
        || !text_value(data.get(value_end + 1..separator)?)
            .is_some_and(|role| valid_role_name(&role))
    {
        return None;
    }
    text_value_allow_empty(data.get(start..value_end)?)
}

fn length_closed_text(data: &[u8], start: usize, length: usize, end: usize) -> Option<String> {
    let value_end = start.checked_add(length)?;
    if length == 0 || value_end >= end || data.get(value_end) != Some(&0xfe) {
        return None;
    }
    text_value(data.get(start..value_end)?)
}

fn text_value(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then_some(())?;
    text_value_allow_empty(bytes)
}

fn text_value_allow_empty(bytes: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(bytes).ok()?;
    value
        .chars()
        .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
        .then(|| value.to_owned())
}

#[cfg(test)]
mod tests;
