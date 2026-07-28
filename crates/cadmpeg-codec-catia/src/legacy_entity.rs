// SPDX-License-Identifier: Apache-2.0
//! Identity framing for the pre-`7C05` design stream.

const CATALOG_OPEN: &[u8] = b"\xde\x04\xfe\xfe\x12CATCatalogManager";
const TEXT_OPEN: &[u8] = b"\xe8\x00\x12\x01";
const SCALAR_OPEN: &[u8] = b"\xfe\x85\x88\x82\xfe";
const NAMED_SCALAR_OPEN: &[u8] = b"\xfe\x84\x88\x82\xfe";
const TYPE_OPEN: &[u8] = b"\xfe\x84\x92\x82";

/// Length production used by one legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyTextEncoding {
    /// Nonzero one-byte inclusive length followed by the text and `FE`.
    U8InclusiveLength,
    /// Zero selector, little-endian `u32` byte length, text, and `FE`.
    ZeroU32Length,
}

/// Schema role selecting one legacy text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyTextRole {
    /// Offset of the role-name length byte.
    pub offset: usize,
    /// Stored UTF-8 role name.
    pub name: String,
    /// Stored selector identity following the role name.
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
    pub role: Option<LegacyTextRole>,
    /// Decoded UTF-8 value.
    pub value: String,
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

/// One stored entity identity in a legacy identity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyEntityIdentity {
    /// Offset of the `EA` identity delimiter.
    pub offset: usize,
    /// Little-endian identity following the delimiter.
    pub entity_id: u32,
}

/// A monotonically identified legacy run terminated by its schema catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyEntityRun {
    /// Offset of the fixed catalog opening production.
    pub catalog_offset: usize,
    /// Stored identities in source order.
    pub identities: Vec<LegacyEntityIdentity>,
    /// Complete schema text fields contained by the identity intervals.
    pub text_fields: Vec<LegacyTextField>,
    /// Complete expression/signature pairs.
    pub relations: Vec<LegacyRelation>,
    /// Complete literal or selector type descriptors.
    pub type_descriptors: Vec<LegacyTypeDescriptor>,
    /// Complete typed scalar packets.
    pub scalar_values: Vec<LegacyScalarValue>,
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
            if bytes[0] != 0xea || bytes[5] != 0x81 {
                return None;
            }
            let entity_id = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
            (entity_id != 0).then_some(LegacyEntityIdentity { offset, entity_id })
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
    let text_fields = identities
        .iter()
        .enumerate()
        .flat_map(|(index, identity)| {
            let start = identity.offset + 6;
            let end = identities
                .get(index + 1)
                .map_or(catalog_offset, |next| next.offset);
            parse_text_fields(data, start, end, identity.entity_id)
        })
        .collect::<Vec<_>>();
    let relations = parse_relations(&text_fields, &identities);
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
    Some(LegacyEntityRun {
        catalog_offset,
        identities,
        text_fields,
        relations,
        type_descriptors,
        scalar_values,
    })
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
        let mut names = fields.iter().filter(|field| {
            field.entity_id == value.entity_id
                && field.role.as_ref().is_some_and(|role| role.name == "name")
        });
        let Some(name) = names.next() else {
            continue;
        };
        if names.next().is_none() {
            value.name_offset = Some(name.offset);
            value.name = Some(name.value.clone());
        }
    }
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
        if let [expression, type_signature] = &fields[start..end] {
            if let Some(signature) = parse_relation_signature(&type_signature.value) {
                relations.push(LegacyRelation {
                    entity_id,
                    parameter_entity_id: relation_parameter_entity(
                        expression,
                        type_signature,
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

fn relation_parameter_entity(
    expression: &LegacyTextField,
    type_signature: &LegacyTextField,
    identities: &[LegacyEntityIdentity],
) -> Option<u32> {
    let owner = expression.role.as_ref()?;
    let parameter = type_signature.role.as_ref()?;
    (owner.name == "body"
        && owner.selector == expression.entity_id
        && parameter.name == "param"
        && identities
            .iter()
            .any(|identity| identity.entity_id == parameter.selector))
    .then_some(parameter.selector)
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
) -> Vec<LegacyTextField> {
    memchr::memmem::find_iter(&data[start..end], TEXT_OPEN)
        .filter_map(|relative| {
            let offset = start + relative;
            let payload = offset + TEXT_OPEN.len();
            parse_text_field(data, payload, end).map(|(encoding, value)| LegacyTextField {
                offset,
                entity_id,
                encoding,
                role: parse_text_role(data, start, offset),
                value,
            })
        })
        .collect()
}

fn parse_text_role(
    data: &[u8],
    interval_start: usize,
    text_offset: usize,
) -> Option<LegacyTextRole> {
    let (selector_start, selector) =
        if text_offset >= interval_start.checked_add(5)? && data[text_offset - 5] == 0x80 {
            (
                text_offset - 5,
                u32::from_le_bytes(data[text_offset - 4..text_offset].try_into().ok()?),
            )
        } else if text_offset >= interval_start.checked_add(2)?
            && (0xd1..=0xe4).contains(&data[text_offset - 2])
        {
            (
                text_offset - 2,
                u32::from(data[text_offset - 2] - 0xd1)
                    .checked_mul(256)?
                    .checked_add(u32::from(data[text_offset - 1]))?
                    .checked_add(1)?,
            )
        } else {
            return None;
        };
    if selector == 0 {
        return None;
    }
    let (offset, name) = (2usize..=u8::MAX as usize).find_map(|inclusive_length| {
        let offset = selector_start.checked_sub(inclusive_length)?;
        if offset < interval_start || usize::from(*data.get(offset)?) != inclusive_length {
            return None;
        }
        let bytes = data.get(offset + 1..selector_start)?;
        text_value(bytes)
            .filter(|name| valid_role_name(name))
            .map(|name| (offset, name))
    })?;
    Some(LegacyTextRole {
        offset,
        name,
        selector,
    })
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
    }
    None
}

fn length_closed_text(data: &[u8], start: usize, length: usize, end: usize) -> Option<String> {
    let value_end = start.checked_add(length)?;
    if length == 0 || value_end >= end || data.get(value_end) != Some(&0xfe) {
        return None;
    }
    text_value(data.get(start..value_end)?)
}

fn text_value(bytes: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(bytes).ok()?;
    (!value.is_empty()
        && value
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r')))
    .then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{parse_runs, CATALOG_OPEN, NAMED_SCALAR_OPEN, SCALAR_OPEN, TEXT_OPEN, TYPE_OPEN};

    fn identity(bytes: &mut Vec<u8>, entity_id: u32) {
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.push(0x81);
        bytes.extend_from_slice(&[0xfd, 0x8c]);
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
        assert!(runs[0].type_descriptors.is_empty());
        assert!(runs[0].scalar_values.is_empty());
        assert_eq!(
            runs[0]
                .identities
                .iter()
                .map(|identity| identity.entity_id)
                .collect::<Vec<_>>(),
            [1, 4, 7]
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
        bytes.extend_from_slice(CATALOG_OPEN);

        let fields = &parse_runs(&bytes)[0].text_fields;
        let body = fields[0].role.as_ref().expect("paged role selector");
        assert_eq!(body.name, "body");
        assert_eq!(body.selector, 4134);
        let parameter = fields[1].role.as_ref().expect("fixed role selector");
        assert_eq!(parameter.name, "param");
        assert_eq!(parameter.selector, 15108);
    }

    #[test]
    fn rejects_unclosed_and_control_bearing_schema_text_candidates() {
        let mut bytes = Vec::new();
        identity(&mut bytes, 1);
        bytes.extend_from_slice(TEXT_OPEN);
        bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0]);
        bytes.extend_from_slice(TEXT_OPEN);
        bytes.extend_from_slice(&[4, b'a', 1, b'b', 0xfe]);
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
        assert_eq!(relation.parameter_entity_id, None);
        assert_eq!(relation.signature.output.as_ref().unwrap().parameter, "#2_");
        assert_eq!(relation.signature.inputs[0].parameter, "#1_");
        assert_eq!(relation.signature.result_type, "VoidType");
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

        assert_eq!(
            parse_runs(&bytes)[0].relations[0].parameter_entity_id,
            Some(4)
        );
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
