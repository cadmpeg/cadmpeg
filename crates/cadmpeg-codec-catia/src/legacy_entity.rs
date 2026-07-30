// SPDX-License-Identifier: Apache-2.0
//! Identity framing for the pre-`7C05` design stream.

const CATALOG_OPEN: &[u8] = b"\xde\x04\xfe\xfe\x12CATCatalogManager";
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

/// Parsed roles in a complete legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRelationSignature {
    /// Ordered input parameters.
    pub inputs: Vec<LegacyRelationParameter>,
    /// Output parameter for a `VoidType` relation.
    pub output: Option<LegacyRelationParameter>,
    /// Source result type.
    pub result_type: String,
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

/// A monotonically identified legacy run terminated by its schema catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyEntityRun {
    /// Offset of the fixed catalog opening production.
    pub catalog_offset: usize,
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
    memchr::memmem::find_iter(data, CATALOG_OPEN)
        .filter_map(|catalog_offset| parse_run_before(data, catalog_offset))
        .collect()
}

fn parse_run_before(data: &[u8], catalog_offset: usize) -> Option<LegacyEntityRun> {
    let mut identities = data[..catalog_offset]
        .windows(6)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            if bytes[0] != 0xea || !matches!(bytes[5], 0x81 | 0x82 | 0xe5 | 0xfd) {
                return None;
            }
            let entity_id = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
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
    bind_scalar_names(&text_fields, &mut scalar_values);
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
    bind_string_names(&text_fields, &mut string_values);
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
    bind_integer_names(&text_fields, &mut integer_values);
    Some(LegacyEntityRun {
        catalog_offset,
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
                    let value = data.get(offset + 6..offset + 14)?;
                    let bits = u64::from_le_bytes(value.try_into().ok()?);
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

fn bind_scalar_names(fields: &[LegacyTextField], values: &mut [LegacyScalarValue]) {
    let mut counts = std::collections::HashMap::new();
    for value in values.iter() {
        *counts.entry(value.entity_id).or_insert(0usize) += 1;
    }
    for value in values {
        if counts.get(&value.entity_id) != Some(&1) {
            continue;
        }
        if let Some(name) = unique_co_owned_name(fields, value.entity_id) {
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

fn bind_string_names(fields: &[LegacyTextField], values: &mut [LegacyStringValue]) {
    let mut counts = std::collections::HashMap::new();
    for value in values.iter() {
        *counts.entry(value.entity_id).or_insert(0usize) += 1;
    }
    for value in values {
        if counts.get(&value.entity_id) != Some(&1) {
            continue;
        }
        if let Some(name) = unique_co_owned_name(fields, value.entity_id) {
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
                    i32::from_le_bytes(data.get(payload + 1..value_end)?.try_into().ok()?),
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

fn bind_integer_names(fields: &[LegacyTextField], values: &mut [LegacyIntegerValue]) {
    let mut counts = std::collections::HashMap::new();
    for value in values.iter() {
        *counts.entry(value.entity_id).or_insert(0usize) += 1;
    }
    for value in values {
        if counts.get(&value.entity_id) != Some(&1) {
            continue;
        }
        if let Some(name) = unique_co_owned_name(fields, value.entity_id) {
            value.name_offset = Some(name.offset);
            value.name = Some(name.value.clone());
        }
    }
}

fn unique_co_owned_name(fields: &[LegacyTextField], entity_id: u32) -> Option<&LegacyTextField> {
    let mut names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("name"))
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
    if (result_type == "VoidType") != output.is_some() {
        return None;
    }
    Some(LegacyRelationSignature {
        inputs,
        output,
        result_type: result_type.to_string(),
    })
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
                field_code: u16::from_le_bytes([*data.get(offset + 1)?, *data.get(offset + 2)?]),
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
                    u32::from_le_bytes(
                        data.get(selector_offset + 1..selector_end)?
                            .try_into()
                            .ok()?,
                    ),
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
            if let Some(role_offset) = field_offset
                .checked_sub(6)
                .filter(|offset| *offset >= start)
            {
                let name_selector = *data.get(role_offset)?;
                if name_selector != 0 && data.get(role_offset + 1) == Some(&0x80) {
                    let selector = u32::from_le_bytes(
                        data.get(role_offset + 2..field_offset)?.try_into().ok()?,
                    );
                    if selector != 0 {
                        return Some(LegacyRoleSelector {
                            offset: role_offset,
                            entity_id,
                            name: LegacyRoleName::Selector(name_selector),
                            encoding: LegacyRoleSelectorEncoding::FixedU32,
                            selector,
                        });
                    }
                }
            }
            if let Some(role_offset) = field_offset
                .checked_sub(3)
                .filter(|offset| *offset >= start)
            {
                let name_selector = *data.get(role_offset)?;
                let page = *data.get(role_offset + 1)?;
                if name_selector != 0 && (0xd1..=0xe4).contains(&page) {
                    let low = *data.get(role_offset + 2)?;
                    return Some(LegacyRoleSelector {
                        offset: role_offset,
                        entity_id,
                        name: LegacyRoleName::Selector(name_selector),
                        encoding: LegacyRoleSelectorEncoding::Paged,
                        selector: u32::from(page - 0xd1)
                            .checked_mul(256)?
                            .checked_add(u32::from(low))?
                            .checked_add(1)?,
                    });
                }
            }
            None
        })
        .collect::<Vec<_>>();
    roles.extend(field_bound_roles);
    roles.sort_by_key(|role| role.offset);
    roles.dedup_by_key(|role| role.offset);
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
        if let Some(length_bytes) = data.get(payload + 1..payload + 5) {
            let length = usize::try_from(u32::from_le_bytes(length_bytes.try_into().ok()?)).ok()?;
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
mod tests {
    use super::{
        parse_runs, LegacyRoleName, LegacyRoleSelector, LegacyRoleSelectorEncoding, CATALOG_OPEN,
        INTEGER_OPEN, NAMED_SCALAR_OPEN, SCALAR_OPEN, STRING_OPEN, TEXT_OPEN, TYPE_OPEN,
    };

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
        };
        assert_eq!(role.end_offset(), None);
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
                .map(|role| (&role.name, role.encoding, role.selector))
                .collect::<Vec<_>>(),
            [
                (
                    &LegacyRoleName::Selector(0xa1),
                    LegacyRoleSelectorEncoding::Paged,
                    4700,
                ),
                (
                    &LegacyRoleName::Selector(0xa2),
                    LegacyRoleSelectorEncoding::Paged,
                    4668,
                ),
                (
                    &LegacyRoleName::Selector(0xa4),
                    LegacyRoleSelectorEncoding::FixedU32,
                    115_925,
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
                .output
                .as_ref()
                .expect("VoidType signature has an output")
                .parameter,
            "#2_"
        );
        assert_eq!(relation.signature.inputs[0].parameter, "#1_");
        assert_eq!(relation.signature.result_type, "VoidType");
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
        assert!(run.text_fields.iter().all(|field| {
            field.encoding == super::LegacyTextEncoding::U8InclusiveLengthE3RoleTail
        }));
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
}
