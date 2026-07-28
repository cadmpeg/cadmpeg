// SPDX-License-Identifier: Apache-2.0
//! Identity framing for the pre-`7C05` design stream.

const CATALOG_OPEN: &[u8] = b"\xde\x04\xfe\xfe\x12CATCatalogManager";
const TEXT_OPEN: &[u8] = b"\xe8\x00\x12\x01";

/// Length production used by one legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyTextEncoding {
    /// Nonzero one-byte inclusive length followed by the text and `FE`.
    U8InclusiveLength,
    /// Zero selector, little-endian `u32` byte length, text, and `FE`.
    ZeroU32Length,
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
    let relations = parse_relations(&text_fields);
    Some(LegacyEntityRun {
        catalog_offset,
        identities,
        text_fields,
        relations,
    })
}

fn parse_relations(fields: &[LegacyTextField]) -> Vec<LegacyRelation> {
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
                value,
            })
        })
        .collect()
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
    use super::{parse_runs, CATALOG_OPEN, TEXT_OPEN};

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
        assert_eq!(relation.signature.output.as_ref().unwrap().parameter, "#2_");
        assert_eq!(relation.signature.inputs[0].parameter, "#1_");
        assert_eq!(relation.signature.result_type, "VoidType");
    }
}
