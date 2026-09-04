// SPDX-License-Identifier: Apache-2.0
//! Framing and identity decode for outer `7C05` entity-table records.

use std::collections::HashSet;

use cadmpeg_core::decode::View;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::value_block;
/// One source-schema selector in a complete `7C06` definition prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DefinitionSchemaSelector {
    /// Stored zero-based source-schema ordinal following `0x32`.
    pub value: u32,
    /// Byte offset of `0x32` within the definition prefix.
    pub offset: usize,
}

/// One fully consumed nullable numeric-pair production in a nested `7C07` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct NumericPair {
    /// Two one-byte compact atoms preceding the nested value frame.
    pub prefix_atoms: [u32; 2],
    /// Two source-ordered nullable scalar slots.
    pub slots: [NumericPairSlot; 2],
}

/// One slot in a complete [`NumericPair`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum NumericPairSlot {
    /// `0xE6` followed by the exact IEEE-754 binary64 bits.
    Binary64 {
        /// Stored little-endian binary64 bits.
        bits: u64,
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// Zero-payload `0xE8` control marker.
    ControlE8 {
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
}

/// Prefix atom of one complete schema-selected `Range` interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum RangeIntervalPrefix {
    /// One compact atom with its exact serialized width.
    Compact {
        /// Decoded compact-atom value.
        value: u32,
        /// Stored width, one or two bytes.
        width: u8,
    },
    /// Exact fixed-width `80 <word:u32le>` form.
    EscapedWord {
        /// Stored little-endian word.
        word: u32,
    },
}

/// One slot in a complete schema-selected `Range` interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum RangeIntervalSlot {
    /// `E6` followed by one finite IEEE-754 binary64 value.
    Binary64 {
        /// Exact stored bits.
        bits: u64,
        /// Byte offset of `E6` within the complete `7C07` payload.
        offset: usize,
    },
    /// Zero-payload `E8` state.
    Unset {
        /// Byte offset of `E8` within the complete `7C07` payload.
        offset: usize,
    },
}

/// Complete encoded value selected by a source-schema entry named `Range`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RangeInterval {
    /// Atom preceding the fixed range type frame.
    pub prefix: RangeIntervalPrefix,
    /// Source-ordered lower and upper slots. An absent pair uses one of the
    /// no-slot productions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<[RangeIntervalSlot; 2]>,
}

/// One item in an embedded numeric value packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum NumericPacketItem {
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

/// Symbol in a reference-signature descriptor program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ReferenceSignatureSymbol {
    /// Symbol `E`.
    E,
    /// Symbol `S`.
    S,
    /// Symbol `T`.
    T,
}

/// Variable prefix form of a complete reference-signature packet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ReferenceSignaturePrefix {
    /// Compact atom `2`.
    #[default]
    Atom2,
    /// Compact atom `35`.
    Atom35,
}

/// One instruction in a complete reference-signature descriptor program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ReferenceSignatureInstruction {
    /// One descriptor symbol: `E`, `S`, or `T`.
    Symbol {
        /// Decoded descriptor symbol.
        symbol: ReferenceSignatureSymbol,
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// One maximal decimal run.
    Decimal {
        /// Exact decimal digits.
        digits: String,
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// Opening parenthesis of a postfix call.
    OpenCall {
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// Comma separating call arguments.
    Comma {
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// Closing parenthesis of a postfix call.
    CloseCall {
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
    /// Postfix hexadecimal selector `#<digit>`.
    Qualifier {
        /// Decoded selector nibble.
        selector: u8,
        /// Byte offset of `#` within the `7C07` payload.
        hash_offset: usize,
        /// Byte offset of the selector digit within the `7C07` payload.
        selector_offset: usize,
    },
    /// Infix difference operator.
    Difference {
        /// Byte offset within the `7C07` payload.
        offset: usize,
    },
}

/// One fully consumed reference-signature production in a nested `7C07` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ReferenceSignature {
    /// First fixed-width reference.
    pub first_reference: u32,
    /// Variable compact atom preceding the nested signature frame.
    #[serde(default)]
    pub prefix: ReferenceSignaturePrefix,
    /// Printable signature bytes between `0x81` and the first terminator.
    pub signature: String,
    /// Source-ordered instruction program spanning the complete signature.
    #[serde(default)]
    pub signature_program: Vec<ReferenceSignatureInstruction>,
    /// Byte offset of the first signature byte within the `7C07` payload.
    #[serde(default)]
    pub signature_offset: usize,
    /// Second fixed-width reference.
    pub second_reference: u32,
    /// Byte offset of the second reference marker within the `7C07` payload.
    #[serde(default)]
    pub second_reference_offset: usize,
}

fn reference_signature_program(
    signature: &str,
    signature_offset: usize,
) -> Option<Vec<ReferenceSignatureInstruction>> {
    let bytes = signature.as_bytes();
    let mut program = Vec::new();
    let mut call_depth = 0_usize;
    let mut expects_operand = true;
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        match bytes[start] {
            byte @ (b'E' | b'S' | b'T') => {
                if !expects_operand {
                    return None;
                }
                let symbol = match byte {
                    b'E' => ReferenceSignatureSymbol::E,
                    b'S' => ReferenceSignatureSymbol::S,
                    b'T' => ReferenceSignatureSymbol::T,
                    _ => unreachable!("matched descriptor symbol"),
                };
                program.push(ReferenceSignatureInstruction::Symbol {
                    symbol,
                    offset: signature_offset + start,
                });
                expects_operand = false;
                at += 1;
            }
            byte if byte.is_ascii_digit() => {
                if !expects_operand {
                    return None;
                }
                at += 1;
                while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    at += 1;
                }
                program.push(ReferenceSignatureInstruction::Decimal {
                    digits: signature[start..at].to_owned(),
                    offset: signature_offset + start,
                });
                expects_operand = false;
            }
            b'(' if !expects_operand => {
                program.push(ReferenceSignatureInstruction::OpenCall {
                    offset: signature_offset + start,
                });
                call_depth = call_depth.checked_add(1)?;
                expects_operand = true;
                at += 1;
            }
            b',' if call_depth != 0 && !expects_operand => {
                program.push(ReferenceSignatureInstruction::Comma {
                    offset: signature_offset + start,
                });
                expects_operand = true;
                at += 1;
            }
            b')' if call_depth != 0 && !expects_operand => {
                program.push(ReferenceSignatureInstruction::CloseCall {
                    offset: signature_offset + start,
                });
                call_depth -= 1;
                expects_operand = false;
                at += 1;
            }
            b'#' if !expects_operand => {
                let selector_offset = start.checked_add(1)?;
                let selector = match *bytes.get(selector_offset)? {
                    byte @ b'0'..=b'9' => byte - b'0',
                    byte @ b'A'..=b'F' => byte - b'A' + 10,
                    _ => return None,
                };
                program.push(ReferenceSignatureInstruction::Qualifier {
                    selector,
                    hash_offset: signature_offset + start,
                    selector_offset: signature_offset + selector_offset,
                });
                at += 2;
            }
            b'-' if !expects_operand => {
                program.push(ReferenceSignatureInstruction::Difference {
                    offset: signature_offset + start,
                });
                expects_operand = true;
                at += 1;
            }
            _ => return None,
        }
    }
    (!expects_operand && call_depth == 0).then_some(program)
}

fn reference_signature_has_one_outer_call(program: &[ReferenceSignatureInstruction]) -> bool {
    if !matches!(
        program,
        [
            ReferenceSignatureInstruction::Decimal { digits, .. },
            ReferenceSignatureInstruction::OpenCall { .. },
            ..,
            ReferenceSignatureInstruction::CloseCall { .. }
        ] if digits == "2"
    ) {
        return false;
    }
    let mut depth = 0_usize;
    for (ordinal, instruction) in program.iter().enumerate().skip(1) {
        match instruction {
            ReferenceSignatureInstruction::OpenCall { .. } => depth += 1,
            ReferenceSignatureInstruction::CloseCall { .. } => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
                if depth == 0 && ordinal + 1 != program.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// One exact packet in a tokenized `7C07` value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum EntityValuePacket {
    /// `<compact_atom> <compact_atom> E8 <selector:u16le> 37 <atom> <atom>
    /// (<E6:f64>|<E7..E9>)+ FE+`.
    Numeric {
        /// Byte offset of the first prefix atom within the value payload.
        offset: usize,
        /// Two one-byte atoms preceding the packet opcode.
        prefix_atoms: [u32; 2],
        /// Stored little-endian numeric-packet selector.
        type_selector: u16,
        /// First one-byte atom after the `0x37` delimiter.
        layout_atom: u32,
        /// Second one-byte atom after the `0x37` delimiter.
        value_atom: u32,
        /// Tagged binary64 values and control markers in serialized order.
        items: Vec<NumericPacketItem>,
        /// Number of consecutive `0xFE` bytes closing the packet.
        terminator_count: usize,
    },
    /// `E8 <value-selector:u16le> 37 FE FE`.
    Compact {
        /// Byte offset of the `E8` opcode within the value payload.
        offset: usize,
        /// Stored little-endian value selector.
        value_selector: u16,
    },
    /// `E9 <type-selector:u16le> <layout:u8> 37 FE FE`.
    Layout {
        /// Byte offset of the opcode within the value payload.
        offset: usize,
        /// Stored little-endian type selector.
        type_selector: u16,
        /// Uninterpreted one-byte layout code.
        layout: u8,
        /// Byte offset of the layout code within the value payload.
        layout_offset: usize,
    },
    /// Exact `83 E9 C0 07 01 E1 E6 <f64le> 88 81{5} 82 E7 81 FE` packet.
    E9Scalar {
        /// Byte offset of the leading `83` atom within the value payload.
        offset: usize,
        /// Byte offset of the scalar's `E6` opcode within the value payload.
        scalar_offset: usize,
        /// Exact stored finite binary64 bits.
        bits: u64,
    },
}

impl EntityValuePacket {
    /// Complete byte range occupied by this packet within its value payload.
    pub(crate) fn byte_range(&self) -> Option<std::ops::Range<usize>> {
        match self {
            Self::Numeric {
                offset,
                items,
                terminator_count,
                ..
            } => {
                let item_end = match items.last()? {
                    NumericPacketItem::Binary64 { offset, .. } => offset.checked_add(9)?,
                    NumericPacketItem::Control { offset, .. } => offset.checked_add(1)?,
                };
                Some(*offset..item_end.checked_add(*terminator_count)?)
            }
            Self::Compact { offset, .. } => Some(*offset..offset.checked_add(6)?),
            Self::Layout { offset, .. } => Some(*offset..offset.checked_add(7)?),
            Self::E9Scalar { offset, .. } => Some(*offset..offset.checked_add(25)?),
        }
    }
}

/// Decode every exact packet in source order from a `7C07` value payload.
#[must_use]
pub fn value_packets(payload: &[u8], fields: &[value_block::ValueField]) -> Vec<EntityValuePacket> {
    let opcode_offsets = fields
        .iter()
        .filter_map(|field| match field {
            value_block::ValueField::Opcode { offset, .. } => Some(*offset),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let e8_opcode_offsets = fields
        .iter()
        .filter_map(|field| match field {
            value_block::ValueField::Opcode { code: 0xe8, offset } => Some(*offset),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let marker_offsets = fields
        .iter()
        .filter_map(|field| match field {
            value_block::ValueField::Marker { code: 0xe8, offset } => Some(*offset),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let atom_offsets = fields
        .iter()
        .filter_map(|field| match field {
            value_block::ValueField::Atom { offset, .. } => Some(*offset),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut packets =
        numeric_value_packets(payload, &e8_opcode_offsets, &marker_offsets, &atom_offsets);
    packets.extend(e9_scalar_packets(payload, &opcode_offsets, &atom_offsets));
    packets.extend(
        (0..payload.len())
            .filter(|index| opcode_offsets.contains(index))
            .filter_map(|index| {
                if let Some([0xe9, _, _, layout, 0x37, 0xfe, 0xfe]) = payload.get(index..index + 7)
                {
                    return Some(EntityValuePacket::Layout {
                        offset: index,
                        type_selector: View::u16_le_at(payload, index + 1)?,
                        layout: *layout,
                        layout_offset: index + 3,
                    });
                }
                match payload.get(index..index + 6) {
                    Some([0xe8, _, _, 0x37, 0xfe, 0xfe]) => Some(EntityValuePacket::Compact {
                        offset: index,
                        value_selector: View::u16_le_at(payload, index + 1)?,
                    }),
                    _ => None,
                }
            }),
    );
    packets.sort_by_key(|packet| match packet {
        EntityValuePacket::Numeric { offset, .. }
        | EntityValuePacket::Compact { offset, .. }
        | EntityValuePacket::Layout { offset, .. }
        | EntityValuePacket::E9Scalar { offset, .. } => *offset,
    });
    packets
}

fn e9_scalar_packets(
    payload: &[u8],
    opcode_offsets: &HashSet<usize>,
    atom_offsets: &HashSet<usize>,
) -> Vec<EntityValuePacket> {
    const PREFIX: [u8; 7] = [0x83, 0xe9, 0xc0, 0x07, 0x01, 0xe1, 0xe6];
    const TRAILER: [u8; 10] = [0x88, 0x81, 0x81, 0x81, 0x81, 0x81, 0x82, 0xe7, 0x81, 0xfe];

    (0..payload.len())
        .filter(|offset| {
            atom_offsets.contains(offset)
                && offset
                    .checked_add(1)
                    .is_some_and(|opcode| opcode_offsets.contains(&opcode))
        })
        .filter_map(|offset| {
            let scalar_offset = offset.checked_add(6)?;
            let scalar_end = scalar_offset.checked_add(9)?;
            let packet_end = offset.checked_add(25)?;
            (payload.get(offset..scalar_offset + 1) == Some(PREFIX.as_slice())
                && payload.get(scalar_end..packet_end) == Some(TRAILER.as_slice()))
            .then_some(())?;
            let bits = View::u64_le_at(payload, scalar_offset + 1)?;
            f64::from_bits(bits)
                .is_finite()
                .then_some(EntityValuePacket::E9Scalar {
                    offset,
                    scalar_offset,
                    bits,
                })
        })
        .collect()
}

fn numeric_value_packets(
    payload: &[u8],
    opcode_offsets: &HashSet<usize>,
    marker_offsets: &HashSet<usize>,
    atom_offsets: &HashSet<usize>,
) -> Vec<EntityValuePacket> {
    let candidates = (0..payload.len())
        .filter(|offset| {
            if !atom_offsets.contains(offset) {
                return false;
            }
            let Some((_, prefix1_offset)) = compact_atom(payload, *offset) else {
                return false;
            };
            if marker_offsets.contains(&prefix1_offset) {
                return true;
            }
            atom_offsets.contains(&prefix1_offset)
                && compact_atom(payload, prefix1_offset)
                    .is_some_and(|(_, opcode_offset)| opcode_offsets.contains(&opcode_offset))
        })
        .filter_map(|offset| parse_numeric_value_packet(payload, offset))
        .collect::<Vec<_>>();
    candidates
        .iter()
        .enumerate()
        .filter(|(index, (_, range))| {
            !candidates
                .iter()
                .enumerate()
                .any(|(other_index, (_, other))| {
                    *index != other_index && range.start < other.end && other.start < range.end
                })
        })
        .map(|(_, (packet, _))| packet.clone())
        .collect()
}

fn parse_numeric_value_packet(
    payload: &[u8],
    offset: usize,
) -> Option<(EntityValuePacket, std::ops::Range<usize>)> {
    let (prefix0, mut at) = compact_atom(payload, offset)?;
    let (prefix1, next) = compact_atom(payload, at)?;
    at = next;
    (payload.get(at) == Some(&0xe8)).then_some(())?;
    let selector = View::u16_le_at(payload, at + 1)?;
    (payload.get(at + 3) == Some(&0x37)).then_some(())?;
    let (layout_atom, next) = one_byte_atom(payload, at + 4)?;
    let (value_atom, next) = one_byte_atom(payload, next)?;
    at = next;
    let mut items = Vec::new();
    let mut binary64_count = 0usize;
    loop {
        match *payload.get(at)? {
            0xe6 => {
                let end = at.checked_add(9)?;
                let bits = View::u64_le_at(payload, at + 1)?;
                items.push(NumericPacketItem::Binary64 { bits, offset: at });
                binary64_count += 1;
                at = end;
            }
            code @ 0xe7..=0xe9 => {
                items.push(NumericPacketItem::Control { code, offset: at });
                at += 1;
            }
            0xfe => break,
            _ => return None,
        }
    }
    (binary64_count != 0).then_some(())?;
    let terminator_start = at;
    while payload.get(at) == Some(&0xfe) {
        at += 1;
    }
    let terminator_count = at - terminator_start;
    if offset == 0 && at == payload.len() && terminator_count >= 2 {
        return None;
    }
    let packet = EntityValuePacket::Numeric {
        offset,
        prefix_atoms: [prefix0, prefix1],
        type_selector: selector,
        layout_atom,
        value_atom,
        items,
        terminator_count,
    };
    let range = packet.byte_range()?;
    (range.end == at).then_some((packet, range))
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
    /// Stored entity identity.
    pub entity_id: u32,
    /// Inline body or nested definition/value frames.
    pub body: EntityBody,
}

/// Body of a length-closed `7C05` entity-table record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityBody {
    /// Complete alternate inline body, including its lead byte.
    Inline(Vec<u8>),
    /// Nested `7C06` definition and `7C07` value frames.
    Nested {
        /// Stored nested `7C06` length.
        definition_len: u32,
        /// Exact definition prefix before the `0xEA` identity delimiter.
        prefix: Vec<u8>,
        /// Source-schema selectors decoded from the complete definition prefix.
        selectors: Vec<DefinitionSchemaSelector>,
        /// Exact definition bytes after the identity.
        suffix: Vec<u8>,
        /// Stored nested `7C07` total length.
        value_len: u32,
        /// Exact nested `7C07` payload.
        value_payload: Vec<u8>,
        /// Exact bytes after the nested `7C07` frame.
        record_suffix: Vec<u8>,
    },
}

impl EntityRecord {
    /// Complete numeric-pair view when the entire value payload has that production.
    #[must_use]
    pub fn numeric_pair(&self) -> Option<NumericPair> {
        match &self.body {
            EntityBody::Nested { value_payload, .. } => parse_numeric_pair(value_payload),
            EntityBody::Inline(_) => None,
        }
    }

    /// Complete reference-signature view when the entire value payload has that production.
    #[must_use]
    pub fn reference_signature(&self) -> Option<ReferenceSignature> {
        match &self.body {
            EntityBody::Nested { value_payload, .. } => parse_reference_signature(value_payload),
            EntityBody::Inline(_) => None,
        }
    }
}

/// Parse every maximal contiguous run of length-closed `7C05` records.
#[must_use]
pub fn parse_runs(data: &[u8]) -> Vec<Vec<EntityRecord>> {
    let mut roots = Vec::new();
    let mut enclosing_end = 0usize;
    for pos in data
        .windows(2)
        .enumerate()
        .filter_map(|(pos, marker)| (marker == [0x7c, 0x05]).then_some(pos))
    {
        let Some(total_len) = u32_le(data, pos + 2).and_then(|len| usize::try_from(len).ok())
        else {
            continue;
        };
        let Some(end) = pos.checked_add(total_len).filter(|end| *end <= data.len()) else {
            continue;
        };
        if end <= enclosing_end {
            continue;
        }
        if let Some(candidate) = parse_candidate_variants(data, pos) {
            enclosing_end = enclosing_end.max(end);
            roots.push(candidate);
        }
    }

    roots
        .into_iter()
        .fold(
            Vec::<Vec<EntityRecordCandidates>>::new(),
            |mut runs, variants| {
                if runs
                    .last()
                    .and_then(|run| run.last())
                    .is_some_and(|last| last.pos.checked_add(last.total_len) == Some(variants.pos))
                {
                    runs.last_mut()
                        .expect("a final record implies a final run")
                        .push(variants);
                } else {
                    runs.push(vec![variants]);
                }
                runs
            },
        )
        .into_iter()
        .filter_map(|run| {
            let identities = unique_monotone_run(&run)?;
            run.iter()
                .zip(identities)
                .map(|(record, identity)| materialize_record(data, record, identity))
                .collect()
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct EntityIdentityCandidate {
    delimiter: usize,
    entity_id: u32,
}

#[derive(Clone)]
struct EntityRecordCandidates {
    pos: usize,
    total_len: usize,
    lead: u8,
    layout: EntityRecordLayout,
    identities: Vec<EntityIdentityCandidate>,
}

#[derive(Clone, Copy)]
enum EntityRecordLayout {
    Nested {
        definition_len: u32,
        definition_end: usize,
        value_len: u32,
        value_end: usize,
    },
    Inline,
}

#[derive(Clone, Copy)]
struct MonotonePathState {
    identity: EntityIdentityCandidate,
    path_count: u8,
    predecessor: Option<usize>,
}

fn unique_monotone_run(records: &[EntityRecordCandidates]) -> Option<Vec<EntityIdentityCandidate>> {
    let first = &records.first()?.identities;
    let mut layers = vec![first
        .iter()
        .copied()
        .map(|identity| MonotonePathState {
            identity,
            path_count: 1,
            predecessor: None,
        })
        .collect::<Vec<_>>()];
    for record in &records[1..] {
        let previous = layers.last().expect("first candidate layer");
        let mut ordered_predecessors = previous.iter().enumerate().collect::<Vec<_>>();
        ordered_predecessors.sort_by_key(|(_, state)| state.identity.entity_id);
        let mut cumulative = Vec::with_capacity(ordered_predecessors.len());
        let mut cumulative_count = 0u8;
        for (index, state) in &ordered_predecessors {
            cumulative_count = cumulative_count.saturating_add(state.path_count).min(2);
            cumulative.push((cumulative_count, (cumulative_count == 1).then_some(*index)));
        }
        let layer = record
            .identities
            .iter()
            .filter_map(|identity| {
                let predecessor_count = ordered_predecessors
                    .partition_point(|(_, state)| state.identity.entity_id < identity.entity_id);
                let (path_count, predecessor) = predecessor_count
                    .checked_sub(1)
                    .map_or((0, None), |index| cumulative[index]);
                (path_count != 0).then_some(MonotonePathState {
                    identity: *identity,
                    path_count,
                    predecessor,
                })
            })
            .collect::<Vec<_>>();
        if layer.is_empty() {
            return None;
        }
        layers.push(layer);
    }
    let final_layer = layers.last()?;
    if final_layer.iter().fold(0u8, |count, state| {
        count.saturating_add(state.path_count).min(2)
    }) != 1
    {
        return None;
    }
    let mut state_index = final_layer.iter().position(|state| state.path_count == 1)?;
    let mut result = Vec::with_capacity(layers.len());
    for layer in layers.iter().rev() {
        let state = &layer[state_index];
        result.push(state.identity);
        if let Some(predecessor) = state.predecessor {
            state_index = predecessor;
        }
    }
    result.reverse();
    Some(result)
}

fn parse_candidate_variants(data: &[u8], pos: usize) -> Option<EntityRecordCandidates> {
    let total_len = usize::try_from(u32_le(data, pos.checked_add(2)?)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 12 || end > data.len() {
        return None;
    }

    let lead = *data.get(pos + 6)?;
    if lead == 0x03 && data.get(pos + 7..pos + 9) != Some(&[0x7c, 0x06]) {
        let identities = identity_candidates(data, pos + 7, end, true);
        return (!identities.is_empty()).then_some(EntityRecordCandidates {
            pos,
            total_len,
            lead,
            layout: EntityRecordLayout::Inline,
            identities,
        });
    }
    if total_len < 19
        || lead > 0x02
        || data.get(pos.checked_add(7)?..pos.checked_add(9)?)? != [0x7c, 0x06]
    {
        return None;
    }
    let definition_len = u32_le(data, pos + 9)?;
    let definition_len_usize = usize::try_from(definition_len).ok()?;
    let definition_end = pos.checked_add(7)?.checked_add(definition_len_usize)?;
    if definition_len_usize < 11 || definition_end > end {
        return None;
    }
    let definition_start = pos + 13;
    let value_len = u32_le(data, definition_end + 2)?;
    let value_len_usize = usize::try_from(value_len).ok()?;
    let value_end = definition_end.checked_add(value_len_usize)?;
    if value_len_usize < 6 || value_end > end {
        return None;
    }
    let identities = identity_candidates(data, definition_start, definition_end, true);
    if data.get(definition_end..definition_end.checked_add(2)?)? != [0x7c, 0x07] {
        return None;
    }
    (!identities.is_empty()).then_some(EntityRecordCandidates {
        pos,
        total_len,
        lead,
        layout: EntityRecordLayout::Nested {
            definition_len,
            definition_end,
            value_len,
            value_end,
        },
        identities,
    })
}

fn identity_candidates(
    data: &[u8],
    start: usize,
    end: usize,
    skip_fixed_fields: bool,
) -> Vec<EntityIdentityCandidate> {
    let mut identities = Vec::new();
    let mut at = start;
    while at < end {
        match data[at] {
            0xea => {
                if at
                    .checked_add(5)
                    .is_some_and(|candidate_end| candidate_end <= end)
                {
                    let Some(entity_id) = u32_le(data, at + 1) else {
                        break;
                    };
                    if entity_id != 0 {
                        identities.push(EntityIdentityCandidate {
                            delimiter: at,
                            entity_id,
                        });
                    }
                }
                at += 1;
            }
            0x32 if at
                .checked_add(5)
                .is_some_and(|candidate_end| candidate_end <= end) =>
            {
                at += 5;
            }
            0xfe if skip_fixed_fields
                && at.checked_add(1).and_then(|next| data.get(next)) == Some(&0xf6)
                && at.checked_add(18).is_some_and(|frame_end| frame_end <= end) =>
            {
                at += 18;
            }
            _ => at += 1,
        }
    }
    identities
}

fn materialize_record(
    data: &[u8],
    candidate: &EntityRecordCandidates,
    identity: EntityIdentityCandidate,
) -> Option<EntityRecord> {
    let record_end = candidate.pos.checked_add(candidate.total_len)?;
    if matches!(candidate.layout, EntityRecordLayout::Inline) {
        return Some(EntityRecord {
            pos: candidate.pos,
            total_len: candidate.total_len,
            lead: candidate.lead,
            entity_id: identity.entity_id,
            body: EntityBody::Inline(data.get(candidate.pos + 6..record_end)?.to_vec()),
        });
    }
    let EntityRecordLayout::Nested {
        definition_len,
        definition_end,
        value_len,
        value_end,
    } = candidate.layout
    else {
        unreachable!("inline entity returned before nested materialization")
    };
    let definition_start = candidate.pos.checked_add(13)?;
    let identity_end = identity.delimiter.checked_add(5)?;
    let value_payload = data.get(definition_end + 6..value_end)?;
    let prefix = data.get(definition_start..identity.delimiter)?;
    Some(EntityRecord {
        pos: candidate.pos,
        total_len: candidate.total_len,
        lead: candidate.lead,
        entity_id: identity.entity_id,
        body: EntityBody::Nested {
            definition_len,
            prefix: prefix.to_vec(),
            selectors: parse_definition_schema_selectors(prefix),
            suffix: data.get(identity_end..definition_end)?.to_vec(),
            value_len,
            value_payload: value_payload.to_vec(),
            record_suffix: data.get(value_end..record_end)?.to_vec(),
        },
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

pub(crate) fn parse_numeric_pair(payload: &[u8]) -> Option<NumericPair> {
    let (prefix0, mut at) = one_byte_atom(payload, 0)?;
    let (prefix1, next) = one_byte_atom(payload, at)?;
    at = next;
    if payload.get(at) != Some(&0xe8) {
        return None;
    }
    at += 1;
    let (type_atom, next) = compact_atom(payload, at)?;
    if type_atom != 4872 {
        return None;
    }
    at = next;
    if payload.get(at) != Some(&0x37) {
        return None;
    }
    at += 1;
    let (layout_atom, next) = one_byte_atom(payload, at)?;
    let (value_atom, next) = one_byte_atom(payload, next)?;
    if layout_atom != 3 || value_atom != 1 {
        return None;
    }
    at = next;

    let mut slots = Vec::with_capacity(2);
    let mut binary64_count = 0;
    for _ in 0..2 {
        let offset = at;
        match *payload.get(at)? {
            0xe6 => {
                let end = at.checked_add(9)?;
                let bits = View::u64_le_at(payload, at + 1)?;
                slots.push(NumericPairSlot::Binary64 { bits, offset });
                binary64_count += 1;
                at = end;
            }
            0xe8 => {
                slots.push(NumericPairSlot::ControlE8 { offset });
                at += 1;
            }
            _ => return None,
        }
    }
    (binary64_count != 0 && payload.get(at..) == Some(&[0xfe, 0xfe])).then_some(NumericPair {
        prefix_atoms: [prefix0, prefix1],
        slots: slots.try_into().ok()?,
    })
}

pub(crate) fn parse_reference_signature(payload: &[u8]) -> Option<ReferenceSignature> {
    if payload.first() != Some(&0x32) {
        return None;
    }
    let first_reference = u32_le(payload, 1)?;
    let (prefix_atom, mut at) = one_byte_atom(payload, 5)?;
    let prefix = match prefix_atom {
        2 => ReferenceSignaturePrefix::Atom2,
        35 => ReferenceSignaturePrefix::Atom35,
        _ => return None,
    };
    if payload.get(at) != Some(&0xe8) {
        return None;
    }
    at += 1;
    let (type_atom, next) = compact_atom(payload, at)?;
    if type_atom != 3851 {
        return None;
    }
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
    let signature_offset = at;
    let signature_bytes = payload.get(signature_offset..signature_end)?;
    if signature_bytes.is_empty()
        || !signature_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    let signature = std::str::from_utf8(signature_bytes).ok()?.to_owned();
    let signature_program = reference_signature_program(&signature, signature_offset)?;
    if !reference_signature_has_one_outer_call(&signature_program)
        || usize::try_from(layout_atom).ok() != signature_bytes.len().checked_add(1)
    {
        return None;
    }
    at = signature_end + 1;
    let second_reference_offset = at;
    if payload.get(at) != Some(&0x32) {
        return None;
    }
    let second_reference = u32_le(payload, at + 1)?;
    if first_reference.checked_add(1) != Some(second_reference) {
        return None;
    }
    let (closing_atom, next) = one_byte_atom(payload, at + 5)?;
    let entity_instruction_count = signature_program
        .iter()
        .filter(|instruction| {
            matches!(
                instruction,
                ReferenceSignatureInstruction::Symbol {
                    symbol: ReferenceSignatureSymbol::E,
                    ..
                }
            )
        })
        .count();
    if usize::try_from(closing_atom).ok() != entity_instruction_count.checked_add(1) {
        return None;
    }
    at = next;
    if payload.get(at) != Some(&0xe9) {
        return None;
    }
    let (closing_type_atom, next) = compact_atom(payload, at + 1)?;
    if closing_type_atom != 3864 {
        return None;
    }
    at = next;
    if payload.get(at..at + 5) != Some(&[0x08, 0x37, 0xfe, 0xfe, 0xfe]) {
        return None;
    }
    (at + 5 == payload.len()).then_some(ReferenceSignature {
        first_reference,
        prefix,
        signature,
        signature_program,
        signature_offset,
        second_reference,
        second_reference_offset,
    })
}

fn one_byte_atom(data: &[u8], at: usize) -> Option<(u32, usize)> {
    let byte = *data.get(at)?;
    match byte {
        0x80..=0xd0 => Some((u32::from(byte - 0x80), at + 1)),
        _ => None,
    }
}

/// Parse one complete encoded value selected by a source-schema `Range`
/// entry. `start` begins after the selector word and `end` is the next
/// catalog-valid selector or the `7C07` payload end.
#[must_use]
pub fn parse_range_interval(payload: &[u8], start: usize, end: usize) -> Option<RangeInterval> {
    let bytes = payload.get(start..end)?;
    let (prefix, mut at) = if bytes.first() == Some(&0x80) && bytes.get(5) == Some(&0xe8) {
        (
            RangeIntervalPrefix::EscapedWord {
                word: u32_le(bytes, 1)?,
            },
            5,
        )
    } else {
        let (value, next) = compact_atom(bytes, 0)?;
        (
            RangeIntervalPrefix::Compact {
                value,
                width: u8::try_from(next).ok()?,
            },
            next,
        )
    };
    (bytes.get(at) == Some(&0xe8)).then_some(())?;
    let (type_atom, next) = compact_atom(bytes, at + 1)?;
    (type_atom == 3848 && bytes.get(next) == Some(&0x37)).then_some(())?;
    let body_at = next.checked_add(1)?;
    let slots = if bytes.get(body_at) == Some(&0xfe) {
        at = body_at;
        None
    } else {
        let (layout, next) = one_byte_atom(bytes, body_at)?;
        at = next;
        match layout {
            1 => None,
            3 => {
                let (value, next) = one_byte_atom(bytes, at)?;
                (value == 1).then_some(())?;
                at = next;
                let mut slots = Vec::with_capacity(2);
                for _ in 0..2 {
                    match *bytes.get(at)? {
                        0xe6 => {
                            let scalar_end = at.checked_add(9)?;
                            let bits = View::u64_le_at(bytes, at + 1)?;
                            f64::from_bits(bits).is_finite().then_some(())?;
                            slots.push(RangeIntervalSlot::Binary64 {
                                bits,
                                offset: start + at,
                            });
                            at = scalar_end;
                        }
                        0xe8 => {
                            slots.push(RangeIntervalSlot::Unset { offset: start + at });
                            at += 1;
                        }
                        _ => return None,
                    }
                }
                Some(slots.try_into().ok()?)
            }
            _ => return None,
        }
    };
    (!bytes[at..].is_empty() && bytes[at..].iter().all(|byte| *byte == 0xfe)).then_some(())?;
    Some(RangeInterval { prefix, slots })
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
    View::u32_le_at(data, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_block;

    fn nested_body(
        record: &EntityRecord,
    ) -> (
        u32,
        &[u8],
        &[DefinitionSchemaSelector],
        &[u8],
        u32,
        &[u8],
        &[u8],
    ) {
        match &record.body {
            EntityBody::Nested {
                definition_len,
                prefix,
                selectors,
                suffix,
                value_len,
                value_payload,
                record_suffix,
            } => (
                *definition_len,
                prefix.as_slice(),
                selectors.as_slice(),
                suffix.as_slice(),
                *value_len,
                value_payload.as_slice(),
                record_suffix.as_slice(),
            ),
            EntityBody::Inline(_) => panic!("expected nested entity body"),
        }
    }

    fn record_with_definition_suffix(
        prefix: &[u8],
        entity_id: u32,
        definition_suffix: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0, 0x7c, 0x06];
        bytes.extend_from_slice(
            &u32::try_from(prefix.len() + definition_suffix.len() + 11)
                .expect("bounded test definition")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(prefix);
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.extend_from_slice(definition_suffix);
        bytes.extend_from_slice(&[0x7c, 0x07, 7, 0, 0, 0, 0xfe, 0xbb]);
        let len = u32::try_from(bytes.len()).expect("bounded test record");
        bytes[2..6].copy_from_slice(&len.to_le_bytes());
        bytes
    }

    fn record(prefix: &[u8], entity_id: u32) -> Vec<u8> {
        record_with_definition_suffix(prefix, entity_id, &[0xaa])
    }

    fn inline_record(prefix: &[u8], entity_id: u32, tail: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0x03];
        bytes.extend_from_slice(prefix);
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.extend_from_slice(tail);
        let len = u32::try_from(bytes.len()).expect("bounded inline test record");
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

        let (_, prefix_bytes, selectors, suffix, value_len, value_payload, record_suffix) =
            nested_body(&run[0]);
        assert_eq!(prefix_bytes, prefix);
        assert_eq!(
            selectors,
            [DefinitionSchemaSelector {
                value: 0x0000_00ea,
                offset: 0,
            }]
        );
        assert_eq!(run[0].entity_id, 37);
        assert_eq!(suffix, [0xaa]);
        assert_eq!(value_len, 7);
        assert_eq!(value_payload, [0xfe]);
        assert_eq!(record_suffix, [0xbb]);
    }

    #[test]
    fn entity_table_run_resolves_a_literal_ea_by_monotone_identity() {
        let mut records = record(&[0x11], 89);
        records.extend(record(&[0xe9, 0xea], 90));
        records.extend(record(&[0x12], 91));

        let runs = parse_runs(&records);
        let [run] = runs.as_slice() else {
            panic!("one uniquely resolved entity-table run");
        };
        assert_eq!(
            run.iter()
                .map(|record| record.entity_id)
                .collect::<Vec<_>>(),
            [89, 90, 91]
        );
        assert_eq!(nested_body(&run[1]).1, [0xe9, 0xea]);
    }

    #[test]
    fn fixed_frame_payload_does_not_create_an_identity_delimiter() {
        let mut first_frame = [0; 18];
        first_frame[..2].copy_from_slice(&[0xfe, 0xf6]);
        first_frame[2] = 0xea;
        first_frame[3..7].copy_from_slice(&100_u32.to_le_bytes());
        let mut second_frame = first_frame;
        second_frame[3..7].copy_from_slice(&101_u32.to_le_bytes());

        let mut records = record_with_definition_suffix(&[0x11], 1, &first_frame);
        records.extend(record_with_definition_suffix(&[0x12], 2, &second_frame));

        let runs = parse_runs(&records);
        let [run] = runs.as_slice() else {
            panic!("one frame-aware entity-table run");
        };
        assert_eq!(
            run.iter()
                .map(|record| record.entity_id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(nested_body(&run[0]).3, first_frame);
        assert_eq!(nested_body(&run[1]).3, second_frame);
    }

    #[test]
    fn inline_fixed_frame_payload_does_not_create_an_identity_delimiter() {
        let mut frame = [0; 18];
        frame[..2].copy_from_slice(&[0xfe, 0xf6]);
        frame[2] = 0xea;
        frame[3..7].copy_from_slice(&100_u32.to_le_bytes());
        let bytes = inline_record(&[0x32, 7, 0, 0, 0], 37, &frame);

        let runs = parse_runs(&bytes);
        let [run] = runs.as_slice() else {
            panic!("one inline entity-table run");
        };
        assert_eq!(run[0].entity_id, 37);
        assert_eq!(run[0].body, EntityBody::Inline(bytes[6..].to_vec()));
    }

    #[test]
    fn entity_table_run_rejects_an_ambiguous_identity_delimiter() {
        assert!(parse_runs(&record(&[0xe9, 0xea], 90)).is_empty());
    }

    #[test]
    fn entity_table_does_not_descend_into_contained_records() {
        let nested = record(&[0x22], 90);
        let mut outer = record(&[0x11], 89);
        outer.extend_from_slice(&nested);
        let outer_len = u32::try_from(outer.len()).expect("bounded outer record");
        outer[2..6].copy_from_slice(&outer_len.to_le_bytes());

        let runs = parse_runs(&outer);
        let [run] = runs.as_slice() else {
            panic!("one outer entity-table run");
        };
        assert_eq!(run.len(), 1);
        assert_eq!(run[0].entity_id, 89);
        let record_suffix = nested_body(&run[0]).6;
        assert_eq!(record_suffix.first(), Some(&0xbb));
        assert_eq!(&record_suffix[1..], nested);
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
    fn numeric_pair_requires_one_complete_nested_production() {
        let payload = [
            0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe8, 0xe6, 0, 0, 0, 0, 0, 0, 0x12,
            0x40, 0xfe, 0xfe,
        ];

        assert_eq!(
            parse_numeric_pair(&payload),
            Some(NumericPair {
                prefix_atoms: [17, 4],
                slots: [
                    NumericPairSlot::ControlE8 { offset: 8 },
                    NumericPairSlot::Binary64 {
                        bits: 4.5_f64.to_bits(),
                        offset: 9,
                    },
                ],
            })
        );
    }

    #[test]
    fn numeric_pair_requires_exact_frame_and_two_nullable_slots() {
        let valid = [
            0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe8, 0xe6, 0, 0, 0, 0, 0, 0, 0x12,
            0x40, 0xfe, 0xfe,
        ];
        for (offset, replacement) in [(4, 0x06), (6, 0x82), (7, 0x82), (8, 0xe7)] {
            let mut malformed = valid;
            malformed[offset] = replacement;
            assert_eq!(parse_numeric_pair(&malformed), None);
        }
        assert_eq!(parse_numeric_pair(&valid[..19]), None);

        let mut extra_slot = valid.to_vec();
        extra_slot.splice(18..18, [0xe8]);
        assert_eq!(parse_numeric_pair(&extra_slot), None);

        let mut all_control = valid.to_vec();
        all_control.splice(9..18, [0xe8]);
        assert_eq!(parse_numeric_pair(&all_control), None);
    }

    #[test]
    fn marker_bytes_in_opaque_regions_do_not_create_numeric_pairs() {
        let opaque = [
            0x73, 0x83, 0xe8, 0xe0, 0x0a, 0x37, 0xd1, 0x51, 0x81, 0x4e, 0x29, 0x42, 0x27, 0x59,
            0xf4, 0xcb, 0x1b, 0x4f, 0xbe, 0x76, 0xaf, 0x2c, 0x10, 0xdf, 0x90, 0xe6, 0, 0, 0, 0, 0,
            0, 0, 0, 0xfe, 0xfe,
        ];

        assert_eq!(parse_numeric_pair(&opaque), None);
    }

    #[test]
    fn range_interval_decodes_nullable_bounds_and_exact_offsets() {
        let mut payload = vec![
            0x32, 0x23, 1, 0, 0, 0x87, 0xe8, 0xe0, 0x07, 0x37, 0x83, 0x81,
        ];
        payload.push(0xe6);
        payload.extend_from_slice(&(-0.2032_f64).to_bits().to_le_bytes());
        payload.push(0xe6);
        payload.extend_from_slice(&0.2032_f64.to_bits().to_le_bytes());
        payload.extend_from_slice(&[0xfe, 0xfe]);

        assert_eq!(
            parse_range_interval(&payload, 5, payload.len()),
            Some(RangeInterval {
                prefix: RangeIntervalPrefix::Compact { value: 7, width: 1 },
                slots: Some([
                    RangeIntervalSlot::Binary64 {
                        bits: (-0.2032_f64).to_bits(),
                        offset: 12,
                    },
                    RangeIntervalSlot::Binary64 {
                        bits: 0.2032_f64.to_bits(),
                        offset: 21,
                    },
                ]),
            })
        );
    }

    #[test]
    fn range_interval_decodes_escaped_prefix_and_unset_bounds() {
        let payload = [
            0x32, 0x23, 1, 0, 0, 0x80, 0x6e, 0x89, 1, 0, 0xe8, 0xe0, 0x07, 0x37, 0x83, 0x81, 0xe8,
            0xe8, 0xfe, 0xfe,
        ];

        assert_eq!(
            parse_range_interval(&payload, 5, payload.len()),
            Some(RangeInterval {
                prefix: RangeIntervalPrefix::EscapedWord { word: 100_718 },
                slots: Some([
                    RangeIntervalSlot::Unset { offset: 16 },
                    RangeIntervalSlot::Unset { offset: 17 },
                ]),
            })
        );
    }

    #[test]
    fn range_interval_requires_the_complete_selected_value() {
        let valid = [0x82, 0xe8, 0xe0, 0x07, 0x37, 0x81, 0xfe];
        assert_eq!(
            parse_range_interval(&valid, 0, valid.len()),
            Some(RangeInterval {
                prefix: RangeIntervalPrefix::Compact { value: 2, width: 1 },
                slots: None,
            })
        );
        for malformed in [
            &valid[..6],
            &[0x82, 0xe8, 0xe0, 0x08, 0x37, 0x81, 0xfe],
            &[0x82, 0xe8, 0xe0, 0x07, 0x37, 0x82, 0xfe],
        ] {
            assert_eq!(parse_range_interval(malformed, 0, malformed.len()), None);
        }
    }

    #[test]
    fn range_interval_decodes_short_no_slot_body() {
        let valid = [0x82, 0xe8, 0xe0, 0x07, 0x37, 0xfe, 0xfe];
        assert_eq!(
            parse_range_interval(&valid, 0, valid.len()),
            Some(RangeInterval {
                prefix: RangeIntervalPrefix::Compact { value: 2, width: 1 },
                slots: None,
            })
        );
        let missing_terminator = [0x82, 0xe8, 0xe0, 0x07, 0x37];
        assert_eq!(
            parse_range_interval(&missing_terminator, 0, missing_terminator.len()),
            None
        );
        let trailing_non_terminator = [0x82, 0xe8, 0xe0, 0x07, 0x37, 0xfe, 0x81];
        assert_eq!(
            parse_range_interval(&trailing_non_terminator, 0, trailing_non_terminator.len()),
            None
        );
    }

    #[test]
    fn reference_signature_requires_one_complete_nested_production() {
        let payload = [
            0x32, 0xcf, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x8c, 0x81, b'2', b'(', b'E', b',',
            b'0', b'(', b'E', b',', b'4', b')', b')', 0xfe, 0x32, 0xd0, 0, 0, 0, 0x83, 0xe9, 0xe0,
            0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
        ];

        assert_eq!(
            parse_reference_signature(&payload),
            Some(ReferenceSignature {
                first_reference: 207,
                prefix: ReferenceSignaturePrefix::Atom2,
                signature: "2(E,0(E,4))".to_owned(),
                signature_program: vec![
                    ReferenceSignatureInstruction::Decimal {
                        digits: "2".to_owned(),
                        offset: 12,
                    },
                    ReferenceSignatureInstruction::OpenCall { offset: 13 },
                    ReferenceSignatureInstruction::Symbol {
                        symbol: ReferenceSignatureSymbol::E,
                        offset: 14,
                    },
                    ReferenceSignatureInstruction::Comma { offset: 15 },
                    ReferenceSignatureInstruction::Decimal {
                        digits: "0".to_owned(),
                        offset: 16,
                    },
                    ReferenceSignatureInstruction::OpenCall { offset: 17 },
                    ReferenceSignatureInstruction::Symbol {
                        symbol: ReferenceSignatureSymbol::E,
                        offset: 18,
                    },
                    ReferenceSignatureInstruction::Comma { offset: 19 },
                    ReferenceSignatureInstruction::Decimal {
                        digits: "4".to_owned(),
                        offset: 20,
                    },
                    ReferenceSignatureInstruction::CloseCall { offset: 21 },
                    ReferenceSignatureInstruction::CloseCall { offset: 22 },
                ],
                signature_offset: 12,
                second_reference: 208,
                second_reference_offset: 24,
            })
        );

        let mut nonconsecutive = payload;
        nonconsecutive[25] += 1;
        assert_eq!(parse_reference_signature(&nonconsecutive), None);

        for (offset, replacement) in [
            (5, 0x83),
            (8, 0x0b),
            (10, 0x8b),
            (12, b'E'),
            (29, 0x82),
            (32, 0x18),
        ] {
            let mut malformed = payload;
            malformed[offset] = replacement;
            assert_eq!(parse_reference_signature(&malformed), None);
        }

        let mut alternate_prefix = payload;
        alternate_prefix[5] = 0xa3;
        assert_eq!(
            parse_reference_signature(&alternate_prefix)
                .expect("alternate reference-signature prefix")
                .prefix,
            ReferenceSignaturePrefix::Atom35
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
    fn reference_signature_requires_balanced_descriptor_delimiters() {
        let payload = [
            0x32, 1, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x84, 0x81, b'E', b')', 0xfe, 0x32, 2,
            0, 0, 0, 0x83, 0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
        ];

        assert_eq!(parse_reference_signature(&payload), None);
    }

    #[test]
    fn reference_signature_program_types_calls_qualifiers_and_differences() {
        let program = super::reference_signature_program("2(E#A(E,3)-0(T))", 12)
            .expect("complete descriptor program");
        assert!(super::reference_signature_has_one_outer_call(&program));
        assert!(program.iter().any(|instruction| matches!(
            instruction,
            ReferenceSignatureInstruction::Qualifier {
                selector: 10,
                hash_offset: 15,
                selector_offset: 16,
            }
        )));
        assert!(program.iter().any(|instruction| matches!(
            instruction,
            ReferenceSignatureInstruction::Difference { offset: 22 }
        )));

        for malformed in [
            "", "(E)", "2()", "2(E,)", "2(#3)", "2(E#G)", "2(E-)", "2(E))", "2 E",
        ] {
            assert_eq!(super::reference_signature_program(malformed, 0), None);
        }
    }

    #[test]
    fn compact_value_packet_requires_the_double_terminated_production() {
        let payload = [0xe8, 0xe0, 0x0a, 0x37, 0xfe, 0xfe, 0xe8, 0x82, 0x37, 0xfe];
        let fields = value_block::tokenize(&payload);
        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::Compact {
                offset: 0,
                value_selector: 0x0ae0,
            }]
        );
    }

    #[test]
    fn compact_value_packet_selector_is_independent_of_token_byte_classes() {
        let payload = [0xe8, 0xf4, 0x1a, 0x37, 0xfe, 0xfe];
        let fields = value_block::tokenize(&payload);
        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::Compact {
                offset: 0,
                value_selector: 0x1af4,
            }]
        );
    }

    #[test]
    fn layout_value_packet_requires_the_layout_and_double_terminator() {
        let payload = [0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe];
        let fields = value_block::tokenize(&payload);
        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::Layout {
                offset: 0,
                type_selector: 0x17e0,
                layout: 8,
                layout_offset: 3,
            }]
        );
    }

    #[test]
    fn layout_value_packet_selector_is_independent_of_token_byte_classes() {
        let payload = [0xe9, 0xf4, 0x17, 0x04, 0x37, 0xfe, 0xfe];
        let fields = value_block::tokenize(&payload);
        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::Layout {
                offset: 0,
                type_selector: 0x17f4,
                layout: 4,
                layout_offset: 3,
            }]
        );
    }

    #[test]
    fn e9_scalar_packet_preserves_the_finite_value_and_offsets() {
        let mut payload = vec![0xaa, 0x83, 0xe9, 0xc0, 0x07, 0x01, 0xe1, 0xe6];
        payload.extend_from_slice(&5.5_f64.to_bits().to_le_bytes());
        payload.extend_from_slice(&[
            0x88, 0x81, 0x81, 0x81, 0x81, 0x81, 0x82, 0xe7, 0x81, 0xfe, 0xbb,
        ]);
        let fields = value_block::tokenize(&payload);

        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::E9Scalar {
                offset: 1,
                scalar_offset: 7,
                bits: 5.5_f64.to_bits(),
            }]
        );
    }

    #[test]
    fn e9_scalar_packet_requires_the_exact_finite_production() {
        let mut valid = vec![0x83, 0xe9, 0xc0, 0x07, 0x01, 0xe1, 0xe6];
        valid.extend_from_slice(&5.5_f64.to_bits().to_le_bytes());
        valid.extend_from_slice(&[0x88, 0x81, 0x81, 0x81, 0x81, 0x81, 0x82, 0xe7, 0x81, 0xfe]);
        for (offset, replacement) in [(0, 0x84), (2, 0xc1), (4, 0x02), (15, 0x89), (23, 0xe8)] {
            let mut malformed = valid.clone();
            malformed[offset] = replacement;
            let fields = value_block::tokenize(&malformed);
            assert!(value_packets(&malformed, &fields).is_empty());
        }

        let fields = value_block::tokenize(&valid[..24]);
        assert!(value_packets(&valid[..24], &fields).is_empty());

        let mut non_finite = valid;
        non_finite[7..15].copy_from_slice(&f64::NAN.to_bits().to_le_bytes());
        let fields = value_block::tokenize(&non_finite);
        assert!(value_packets(&non_finite, &fields).is_empty());
    }

    #[test]
    fn e9_scalar_bytes_inside_a_byte_string_do_not_create_a_packet() {
        let mut payload = vec![0xe5, 25, 0, 0, 0, 0x83, 0xe9, 0xc0, 0x07, 0x01, 0xe1, 0xe6];
        payload.extend_from_slice(&5.5_f64.to_bits().to_le_bytes());
        payload.extend_from_slice(&[0x88, 0x81, 0x81, 0x81, 0x81, 0x81, 0x82, 0xe7, 0x81, 0xfe]);
        payload.push(0xfe);
        let fields = value_block::tokenize(&payload);

        assert!(value_packets(&payload, &fields).is_empty());
    }

    #[test]
    fn packet_shaped_bytes_inside_a_typed_value_do_not_create_packets() {
        let payload = [0x87, 0xe6, 0xe8, 1, 2, 0x37, 0xfe, 0xfe, 0, 0];
        let fields = value_block::tokenize(&payload);
        assert!(value_packets(&payload, &fields).is_empty());
    }

    #[test]
    fn numeric_packet_opcode_inside_binary64_does_not_create_a_packet() {
        let mut payload = vec![
            0x87, 0xe6, 0x81, 0x82, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe6,
        ];
        payload.extend_from_slice(&42.0_f64.to_bits().to_le_bytes());
        payload.push(0xfe);
        let fields = value_block::tokenize(&payload);

        assert!(value_packets(&payload, &fields).is_empty());
    }

    #[test]
    fn embedded_numeric_value_packet_preserves_exact_values_and_controls() {
        let mut payload = vec![0xaa, 0x81, 0x87, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84];
        payload.push(0xe7);
        payload.push(0xe6);
        payload.extend_from_slice(&(-32.0_f64).to_bits().to_le_bytes());
        payload.push(0xe9);
        payload.push(0xe6);
        payload.extend_from_slice(&180.902_997_326_510_7_f64.to_bits().to_le_bytes());
        payload.extend_from_slice(&[0xfe, 0xbb]);
        let fields = value_block::tokenize(&payload);

        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::Numeric {
                offset: 1,
                prefix_atoms: [1, 7],
                type_selector: 0x1af4,
                layout_atom: 3,
                value_atom: 4,
                items: vec![
                    NumericPacketItem::Control {
                        code: 0xe7,
                        offset: 9,
                    },
                    NumericPacketItem::Binary64 {
                        bits: (-32.0_f64).to_bits(),
                        offset: 10,
                    },
                    NumericPacketItem::Control {
                        code: 0xe9,
                        offset: 19,
                    },
                    NumericPacketItem::Binary64 {
                        bits: 180.902_997_326_510_7_f64.to_bits(),
                        offset: 20,
                    },
                ],
                terminator_count: 1,
            }]
        );
    }

    #[test]
    fn embedded_numeric_value_packet_accepts_compact_prefix_atoms() {
        let mut payload = vec![
            0x88, 0xd5, 0x3f, 0xe8, 0xe4, 0x07, 0x37, 0x88, 0x81, 0xe8, 0xe8, 0xe8, 0xe6,
        ];
        payload.extend_from_slice(&12.7_f64.to_bits().to_le_bytes());
        payload.extend_from_slice(&[0xe8, 0xe6]);
        payload.extend_from_slice(&std::f64::consts::PI.to_bits().to_le_bytes());
        payload.extend_from_slice(&[0xe7, 0xfe]);
        let fields = value_block::tokenize(&payload);

        assert_eq!(
            value_packets(&payload, &fields),
            [EntityValuePacket::Numeric {
                offset: 0,
                prefix_atoms: [8, 1088],
                type_selector: 0x07e4,
                layout_atom: 8,
                value_atom: 1,
                items: vec![
                    NumericPacketItem::Control {
                        code: 0xe8,
                        offset: 9,
                    },
                    NumericPacketItem::Control {
                        code: 0xe8,
                        offset: 10,
                    },
                    NumericPacketItem::Control {
                        code: 0xe8,
                        offset: 11,
                    },
                    NumericPacketItem::Binary64 {
                        bits: 12.7_f64.to_bits(),
                        offset: 12,
                    },
                    NumericPacketItem::Control {
                        code: 0xe8,
                        offset: 21,
                    },
                    NumericPacketItem::Binary64 {
                        bits: std::f64::consts::PI.to_bits(),
                        offset: 22,
                    },
                    NumericPacketItem::Control {
                        code: 0xe7,
                        offset: 31,
                    },
                ],
                terminator_count: 1,
            }]
        );
    }

    #[test]
    fn malformed_numeric_value_packets_are_not_assigned() {
        for payload in [
            vec![0x81, 0x82, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe6, 0, 0],
            vec![0x81, 0x82, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe7, 0xfe],
            vec![
                0x81, 0x82, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe6, 0, 0, 0, 0, 0, 0, 0, 0, 0xaa,
            ],
        ] {
            let fields = value_block::tokenize(&payload);
            assert!(value_packets(&payload, &fields).is_empty());
        }
    }

    #[test]
    fn complete_numeric_pair_is_not_duplicated_as_an_embedded_packet() {
        let mut payload = vec![0x81, 0x82, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6];
        payload.extend_from_slice(&42.0_f64.to_bits().to_le_bytes());
        payload.extend_from_slice(&[0xe8, 0xfe, 0xfe]);
        let fields = value_block::tokenize(&payload);

        assert!(parse_numeric_pair(&payload).is_some());
        assert!(value_packets(&payload, &fields).is_empty());
    }
}
