// SPDX-License-Identifier: Apache-2.0
//! Framing and identity decode for outer `7C05` entity-table records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::value_block;

/// One source-schema selector in a complete `7C06` definition prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DefinitionSchemaSelector {
    /// Stored zero-based source-schema ordinal following `0x32`.
    pub value: u32,
    /// Byte offset of `0x32` within the definition prefix.
    pub offset: usize,
}

/// One fully consumed numeric-tuple production in a nested `7C07` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NumericTuple {
    /// Two one-byte compact atoms preceding the nested value frame.
    pub prefix_atoms: [u32; 2],
    /// Compact nested-frame type atom following `0xE8`.
    pub type_atom: u32,
    /// First one-byte compact atom after the `0x37` delimiter.
    pub layout_atom: u32,
    /// Second one-byte compact atom after the `0x37` delimiter.
    pub value_atom: u32,
    /// Tagged numeric values and control markers in serialized order.
    pub items: Vec<NumericTupleItem>,
}

/// One item in a complete [`NumericTuple`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum NumericTupleItem {
    /// `0xE6` followed by the exact IEEE-754 binary64 bits.
    Binary64 {
        /// Stored little-endian binary64 bits.
        bits: u64,
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// One zero-payload control marker in `0xE7..=0xE9`.
    Control {
        /// Stored control code.
        code: u8,
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
}

/// One fully consumed reference-signature production in a nested `7C07` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReferenceSignature {
    /// First fixed-width reference.
    pub first_reference: u32,
    /// Compact atom preceding the nested signature frame.
    pub prefix_atom: u32,
    /// Compact nested-frame type atom following the first `0xE8`.
    pub type_atom: u32,
    /// One-byte layout atom following the first `0x37`.
    pub layout_atom: u32,
    /// Printable signature bytes between `0x81` and the first terminator.
    pub signature: String,
    /// Second fixed-width reference.
    pub second_reference: u32,
    /// One-byte atom preceding the closing nested frame.
    pub closing_atom: u32,
    /// Compact closing-frame type atom following `0xE9`.
    pub closing_type_atom: u32,
}

/// One exact packet in a tokenized `7C07` value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EntityValuePacket {
    /// `E8 <value:compact-atom> 37 FE FE`.
    Compact {
        /// Byte offset of the `E8` opcode within the value payload.
        offset: usize,
        /// Decoded compact atom.
        value: u32,
        /// Stored width of the compact atom.
        width: u8,
    },
    /// `<opcode:E8|E9> <type:compact-atom> <layout:u8> 37 FE FE`.
    Layout {
        /// Stored packet opcode, either `E8` or `E9`.
        opcode: u8,
        /// Byte offset of the opcode within the value payload.
        offset: usize,
        /// Decoded compact type atom.
        type_atom: u32,
        /// Stored width of the type atom.
        type_width: u8,
        /// Uninterpreted one-byte layout code.
        layout: u8,
        /// Byte offset of the layout code within the value payload.
        layout_offset: usize,
    },
}

/// Decode every exact packet in source order from a tokenized value payload.
#[must_use]
pub fn value_packets(fields: &[value_block::ValueField]) -> Vec<EntityValuePacket> {
    (0..fields.len())
        .filter_map(|index| {
            if let Some(
                [
                    value_block::ValueField::Opcode { code, offset },
                    value_block::ValueField::Atom {
                        value: type_atom,
                        width: type_width,
                        ..
                    },
                    layout,
                    value_block::ValueField::Separator { .. },
                    value_block::ValueField::Terminator { .. },
                    value_block::ValueField::Terminator { .. },
                ],
            ) = fields.get(index..index + 6)
            {
                if matches!(code, 0xe8 | 0xe9) {
                    let layout = match layout {
                        value_block::ValueField::Atom {
                            value,
                            width: 1,
                            offset,
                        } => Some((u8::try_from(value.checked_add(0x80)?).ok()?, *offset)),
                        value_block::ValueField::Literal { value, offset } => {
                            Some((*value, *offset))
                        }
                        _ => None,
                    };
                    if let Some((layout, layout_offset)) = layout {
                        return Some(EntityValuePacket::Layout {
                            opcode: *code,
                            offset: *offset,
                            type_atom: *type_atom,
                            type_width: *type_width,
                            layout,
                            layout_offset,
                        });
                    }
                }
            }
            match fields.get(index..index + 5) {
                Some([
                    value_block::ValueField::Opcode { code: 0xe8, offset },
                    value_block::ValueField::Atom { value, width, .. },
                    value_block::ValueField::Separator { .. },
                    value_block::ValueField::Terminator { .. },
                    value_block::ValueField::Terminator { .. },
                ]) => Some(EntityValuePacket::Compact {
                    offset: *offset,
                    value: *value,
                    width: *width,
                }),
                _ => None,
            }
        })
        .collect()
}

/// One length-closed `7C05` entity-table record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord {
    /// Byte offset of the `7C05` marker.
    pub pos: usize,
    /// Total framed byte length.
    pub total_len: usize,
    /// Byte between the `7C05` length and nested `7C06` marker.
    pub lead: u8,
    /// Stored nested `7C06` length.
    pub definition_len: u32,
    /// Exact definition prefix before the `0xEA` identity delimiter.
    pub definition_prefix: Vec<u8>,
    /// Source-schema selectors decoded from the complete definition prefix.
    pub definition_schema_selectors: Vec<DefinitionSchemaSelector>,
    /// Stored entity identity.
    pub entity_id: u32,
    /// Exact definition bytes after the identity.
    pub definition_suffix: Vec<u8>,
    /// Stored nested `7C07` total length.
    pub value_len: u32,
    /// Exact nested `7C07` payload.
    pub value_payload: Vec<u8>,
    /// Complete numeric-tuple view when the entire value payload has that production.
    pub numeric_tuple: Option<NumericTuple>,
    /// Complete reference-signature view when the entire value payload has that production.
    pub reference_signature: Option<ReferenceSignature>,
    /// Exact bytes after the nested `7C07` frame.
    pub record_suffix: Vec<u8>,
}

/// Parse every maximal contiguous run of length-closed `7C05` records.
#[must_use]
pub fn parse_runs(data: &[u8]) -> Vec<Vec<EntityRecord>> {
    let candidates = data
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x05])
        .filter_map(|(pos, _)| parse_candidate(data, pos))
        .collect::<Vec<_>>();
    let roots = candidates
        .iter()
        .filter(|candidate| {
            !candidates.iter().any(|outer| {
                outer.pos < candidate.pos
                    && outer.pos.checked_add(outer.total_len).is_some_and(|end| {
                        candidate
                            .pos
                            .checked_add(candidate.total_len)
                            .is_some_and(|candidate_end| candidate_end <= end)
                    })
            })
        })
        .cloned()
        .collect::<Vec<_>>();

    roots
        .into_iter()
        .fold(Vec::<Vec<EntityRecord>>::new(), |mut runs, record| {
            if runs
                .last()
                .and_then(|run| run.last())
                .is_some_and(|last| last.pos.checked_add(last.total_len) == Some(record.pos))
            {
                runs.last_mut()
                    .expect("a final record implies a final run")
                    .push(record);
            } else {
                runs.push(vec![record]);
            }
            runs
        })
        .into_iter()
        .filter(|run| {
            run.windows(2)
                .all(|pair| pair[0].entity_id < pair[1].entity_id)
        })
        .collect()
}

fn parse_candidate(data: &[u8], pos: usize) -> Option<EntityRecord> {
    let total_len = usize::try_from(u32_le(data, pos.checked_add(2)?)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 19
        || end > data.len()
        || data.get(pos.checked_add(6)?)? > &0x02
        || data.get(pos.checked_add(7)?..pos.checked_add(9)?)? != [0x7c, 0x06]
    {
        return None;
    }

    let lead = *data.get(pos + 6)?;
    let definition_len = u32_le(data, pos + 9)?;
    let definition_len_usize = usize::try_from(definition_len).ok()?;
    let definition_end = pos.checked_add(7)?.checked_add(definition_len_usize)?;
    if definition_len_usize < 11 || definition_end > end {
        return None;
    }
    let definition_start = pos + 13;
    let mut at = definition_start;
    while at < definition_end {
        match data[at] {
            0xea => break,
            0x32 => at = at.checked_add(5)?,
            _ => at += 1,
        }
    }
    let identity_end = at.checked_add(5)?;
    if identity_end > definition_end
        || data.get(definition_end..definition_end.checked_add(2)?)? != [0x7c, 0x07]
    {
        return None;
    }
    let entity_id = u32_le(data, at + 1)?;
    let value_len = u32_le(data, definition_end + 2)?;
    let value_len_usize = usize::try_from(value_len).ok()?;
    let value_end = definition_end.checked_add(value_len_usize)?;
    if entity_id == 0 || value_len_usize < 6 || value_end > end {
        return None;
    }

    Some(EntityRecord {
        pos,
        total_len,
        lead,
        definition_len,
        definition_prefix: data[definition_start..at].to_vec(),
        definition_schema_selectors: parse_definition_schema_selectors(&data[definition_start..at]),
        entity_id,
        definition_suffix: data[identity_end..definition_end].to_vec(),
        value_len,
        value_payload: data[definition_end + 6..value_end].to_vec(),
        numeric_tuple: parse_numeric_tuple(&data[definition_end + 6..value_end]),
        reference_signature: parse_reference_signature(&data[definition_end + 6..value_end]),
        record_suffix: data[value_end..end].to_vec(),
    })
}

pub(crate) fn parse_definition_schema_selectors(prefix: &[u8]) -> Vec<DefinitionSchemaSelector> {
    let mut selectors = Vec::new();
    let mut at = 0;
    while at < prefix.len() {
        if prefix.get(at) == Some(&0x32) && at.checked_add(5).is_some_and(|end| end <= prefix.len())
        {
            selectors.push(DefinitionSchemaSelector {
                value: u32_le(prefix, at + 1).expect("checked definition atom extent"),
                offset: at,
            });
            at += 5;
        } else {
            at += 1;
        }
    }
    selectors
}

pub(crate) fn parse_numeric_tuple(payload: &[u8]) -> Option<NumericTuple> {
    let (prefix0, mut at) = one_byte_atom(payload, 0)?;
    let (prefix1, next) = one_byte_atom(payload, at)?;
    at = next;
    if payload.get(at) != Some(&0xe8) {
        return None;
    }
    at += 1;
    let (type_atom, next) = compact_atom(payload, at)?;
    at = next;
    if payload.get(at) != Some(&0x37) {
        return None;
    }
    at += 1;
    let (layout_atom, next) = one_byte_atom(payload, at)?;
    let (value_atom, next) = one_byte_atom(payload, next)?;
    at = next;

    let mut items = Vec::new();
    let mut binary64_count = 0;
    while payload.get(at..at.checked_add(2)?) != Some(&[0xfe, 0xfe]) {
        let offset = at;
        match *payload.get(at)? {
            0xe6 => {
                let end = at.checked_add(9)?;
                let bits = u64::from_le_bytes(payload.get(at + 1..end)?.try_into().ok()?);
                items.push(NumericTupleItem::Binary64 { bits, offset });
                binary64_count += 1;
                at = end;
            }
            code @ 0xe7..=0xe9 => {
                items.push(NumericTupleItem::Control { code, offset });
                at += 1;
            }
            _ => return None,
        }
    }
    (binary64_count != 0 && at + 2 == payload.len()).then_some(NumericTuple {
        prefix_atoms: [prefix0, prefix1],
        type_atom,
        layout_atom,
        value_atom,
        items,
    })
}

pub(crate) fn parse_reference_signature(payload: &[u8]) -> Option<ReferenceSignature> {
    if payload.first() != Some(&0x32) {
        return None;
    }
    let first_reference = u32_le(payload, 1)?;
    let (prefix_atom, mut at) = one_byte_atom(payload, 5)?;
    if payload.get(at) != Some(&0xe8) {
        return None;
    }
    at += 1;
    let (type_atom, next) = compact_atom(payload, at)?;
    at = next;
    if payload.get(at) != Some(&0x37) {
        return None;
    }
    let (layout_atom, next) = one_byte_atom(payload, at + 1)?;
    at = next;
    if payload.get(at) != Some(&0x81) {
        return None;
    }
    at += 1;
    let signature_end = payload.get(at..)?.iter().position(|byte| *byte == 0xfe)? + at;
    let signature_bytes = payload.get(at..signature_end)?;
    if signature_bytes.is_empty()
        || !signature_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    let signature = std::str::from_utf8(signature_bytes).ok()?.to_owned();
    at = signature_end + 1;
    if payload.get(at) != Some(&0x32) {
        return None;
    }
    let second_reference = u32_le(payload, at + 1)?;
    let (closing_atom, next) = one_byte_atom(payload, at + 5)?;
    at = next;
    if payload.get(at) != Some(&0xe9) {
        return None;
    }
    let (closing_type_atom, next) = compact_atom(payload, at + 1)?;
    at = next;
    if payload.get(at..at + 5) != Some(&[0x08, 0x37, 0xfe, 0xfe, 0xfe]) {
        return None;
    }
    (at + 5 == payload.len()).then_some(ReferenceSignature {
        first_reference,
        prefix_atom,
        type_atom,
        layout_atom,
        signature,
        second_reference,
        closing_atom,
        closing_type_atom,
    })
}

fn one_byte_atom(data: &[u8], at: usize) -> Option<(u32, usize)> {
    let byte = *data.get(at)?;
    match byte {
        0x80..=0xd0 => Some((u32::from(byte - 0x80), at + 1)),
        _ => None,
    }
}

fn compact_atom(data: &[u8], at: usize) -> Option<(u32, usize)> {
    let byte = *data.get(at)?;
    match byte {
        0x80..=0xd0 => Some((u32::from(byte - 0x80), at + 1)),
        0xd1..=0xe4 => Some((
            u32::from(byte - 0xd1) * 256 + u32::from(*data.get(at + 1)?) + 1,
            at + 2,
        )),
        _ => None,
    }
}

fn u32_le(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_definition_schema_selectors, parse_numeric_tuple, parse_reference_signature,
        parse_runs, value_packets, DefinitionSchemaSelector, EntityValuePacket, NumericTuple,
        NumericTupleItem, ReferenceSignature,
    };
    use crate::value_block;

    fn record(prefix: &[u8], entity_id: u32) -> Vec<u8> {
        let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0, 0x7c, 0x06];
        bytes.extend_from_slice(
            &u32::try_from(prefix.len() + 12)
                .expect("bounded test definition")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(prefix);
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.push(0xaa);
        bytes.extend_from_slice(&[0x7c, 0x07, 7, 0, 0, 0, 0xfe, 0xbb]);
        let len = u32::try_from(bytes.len()).expect("bounded test record");
        bytes[2..6].copy_from_slice(&len.to_le_bytes());
        bytes
    }

    #[test]
    fn fixed_width_definition_atom_does_not_terminate_at_embedded_ea() {
        let prefix = [0x32, 0xea, 0, 0, 0, 0x11];
        let records = record(&prefix, 37);
        let runs = parse_runs(&records);
        let [run] = runs.as_slice() else {
            panic!("one entity-table run");
        };

        assert_eq!(run[0].definition_prefix, prefix);
        assert_eq!(
            run[0].definition_schema_selectors,
            [DefinitionSchemaSelector {
                value: 0x0000_00ea,
                offset: 0,
            }]
        );
        assert_eq!(run[0].entity_id, 37);
        assert_eq!(run[0].definition_suffix, [0xaa]);
        assert_eq!(run[0].value_len, 7);
        assert_eq!(run[0].value_payload, [0xfe]);
        assert_eq!(run[0].record_suffix, [0xbb]);
    }

    #[test]
    fn truncated_definition_selector_is_not_assigned() {
        assert!(parse_definition_schema_selectors(&[0x32, 1, 2, 3]).is_empty());
    }

    #[test]
    fn entity_table_runs_require_strictly_increasing_identities() {
        let mut records = record(&[0x11], 3);
        records.extend(record(&[0x12], 2));

        assert!(parse_runs(&records).is_empty());
    }

    #[test]
    fn numeric_tuple_requires_one_complete_nested_production() {
        let payload = [
            0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe8, 0xe6, 0, 0, 0, 0, 0, 0, 0x12,
            0x40, 0xfe, 0xfe,
        ];

        assert_eq!(
            parse_numeric_tuple(&payload),
            Some(NumericTuple {
                prefix_atoms: [17, 4],
                type_atom: 4872,
                layout_atom: 3,
                value_atom: 1,
                items: vec![
                    NumericTupleItem::Control {
                        code: 0xe8,
                        offset: 8,
                    },
                    NumericTupleItem::Binary64 {
                        bits: 4.5_f64.to_bits(),
                        offset: 9,
                    },
                ],
            })
        );
    }

    #[test]
    fn marker_bytes_in_opaque_regions_do_not_create_numeric_tuples() {
        let opaque = [
            0x73, 0x83, 0xe8, 0xe0, 0x0a, 0x37, 0xd1, 0x51, 0x81, 0x4e, 0x29, 0x42, 0x27, 0x59,
            0xf4, 0xcb, 0x1b, 0x4f, 0xbe, 0x76, 0xaf, 0x2c, 0x10, 0xdf, 0x90, 0xe6, 0, 0, 0, 0, 0,
            0, 0, 0, 0xfe, 0xfe,
        ];

        assert_eq!(parse_numeric_tuple(&opaque), None);
    }

    #[test]
    fn reference_signature_requires_one_complete_nested_production() {
        let payload = [
            0x32, 0xcf, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x8c, 0x81, b'(', b'E', b',', b'0',
            b'(', b'E', b',', b'4', b')', b')', 0xfe, 0x32, 0xd0, 0, 0, 0, 0x83, 0xe9, 0xe0, 0x17,
            0x08, 0x37, 0xfe, 0xfe, 0xfe,
        ];

        assert_eq!(
            parse_reference_signature(&payload),
            Some(ReferenceSignature {
                first_reference: 207,
                prefix_atom: 2,
                type_atom: 3851,
                layout_atom: 12,
                signature: "(E,0(E,4))".to_owned(),
                second_reference: 208,
                closing_atom: 3,
                closing_type_atom: 3864,
            })
        );
    }

    #[test]
    fn embedded_reference_markers_do_not_create_reference_signatures() {
        let payload = [
            0x90, 0x32, 0xcf, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x8c, 0x81, b'(', b'E', b')',
            0xfe, 0x32, 0xd0, 0, 0, 0, 0x83, 0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
        ];

        assert_eq!(parse_reference_signature(&payload), None);
    }

    #[test]
    fn compact_value_packet_requires_the_double_terminated_production() {
        let fields =
            value_block::tokenize(&[0xe8, 0xe0, 0x0a, 0x37, 0xfe, 0xfe, 0xe8, 0x82, 0x37, 0xfe]);
        assert_eq!(
            value_packets(&fields),
            [EntityValuePacket::Compact {
                offset: 0,
                value: 3851,
                width: 2,
            }]
        );
    }

    #[test]
    fn layout_value_packet_requires_the_layout_and_double_terminator() {
        let fields = value_block::tokenize(&[0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe]);
        assert_eq!(
            value_packets(&fields),
            [EntityValuePacket::Layout {
                opcode: 0xe9,
                offset: 0,
                type_atom: 3864,
                type_width: 2,
                layout: 8,
                layout_offset: 3,
            }]
        );
    }
}
