// SPDX-License-Identifier: Apache-2.0
//! Frame NX object-model entities using external boundary and identity arrays.

use std::collections::BTreeSet;

use cadmpeg_ir::le::u32_at;

/// One NX object-model entity with persistent object identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord<'a> {
    /// NX object identifier paired with this boundary slot, when the section
    /// carries a fixed-width object-id table.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// One length-framed NX object-model class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition<'a> {
    /// Absolute byte offset of the definition's length byte.
    pub offset: usize,
    /// Registered `UGS::` class name.
    pub name: &'a str,
    /// Declaration code following the name.
    pub trailing_code: u8,
    /// Bytes between this declaration core and the next class declaration.
    pub registry_suffix: &'a [u8],
}

/// One member declaration in an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition<'a> {
    /// Offset of the declaration length byte.
    pub offset: usize,
    /// Registered `m_` member name.
    pub name: &'a str,
    /// Declaration code immediately following the name.
    pub trailing_code: u8,
    /// Bytes between this declaration core and the next member declaration.
    pub registry_suffix: &'a [u8],
}

/// One self-framed printable string value in an NX OM entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringValue<'a> {
    /// Absolute byte offset of the `66 32 03` marker.
    pub offset: usize,
    /// Printable value bytes.
    pub value: &'a str,
}

/// Self-framed NX product/version marker in an OM store root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreVersion<'a> {
    /// Absolute offset of the `04 01` marker.
    pub offset: usize,
    /// Exact printable product/version text, including the `NX ` prefix.
    pub value: &'a str,
}

/// Header of an internally pointed size-framed OM record area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordAreaHeader<'a> {
    /// Absolute offset of the first control word.
    pub offset: usize,
    /// Three little-endian control words preceding the product record.
    pub control_words: [u32; 3],
    /// Product/version record following the control words.
    pub product: StoreVersion<'a>,
}

/// Tagged NX OM cross-record reference family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// `e0` marker followed by a 32-bit big-endian persistent handle.
    PersistentHandle,
    /// Four-byte word whose high nibble is `c` and low 28 bits are the value.
    Tagged28,
    /// `90` marker followed by a 16-bit big-endian record ordinal.
    RecordOrdinal16,
}

/// One value in an NX OM compact-index lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactIndex {
    /// `ff` null/sentinel entry.
    Null,
    /// Decoded non-null index.
    Value(u32),
}

/// Decode a complete NX OM compact-index lane.
///
/// `00..7f` are direct values, `80..fe` introduce one low byte, and `ff` is
/// null. A dangling two-byte prefix rejects the whole lane.
pub fn compact_indices(bytes: &[u8]) -> Option<Vec<CompactIndex>> {
    let mut values = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let prefix = bytes[at];
        at += 1;
        if prefix == 0xff {
            values.push(CompactIndex::Null);
        } else if prefix >= 0x80 {
            let low = u32::from(*bytes.get(at)?);
            at += 1;
            values.push(CompactIndex::Value(u32::from(prefix - 0x80) * 256 + low));
        } else {
            values.push(CompactIndex::Value(u32::from(prefix)));
        }
    }
    Some(values)
}

/// One tagged reference occurrence in an externally bounded OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceValue {
    /// Absolute byte offset of the reference marker.
    pub offset: usize,
    /// Reference family.
    pub kind: ReferenceKind,
    /// Unsigned reference value without its marker/tag bits.
    pub value: u32,
}

/// Unit declared by an NX numeric-expression serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as serialized by NX.
    Degree,
}

/// One numeric expression decoded from an exactly bounded OM entity.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericExpression<'a> {
    /// Persistent identity of the containing OM entity, when indexed.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the expression text.
    pub offset: usize,
    /// NX parameter name.
    pub name: &'a str,
    /// Decimal identifier following the leading `p`, when present.
    pub parameter_index: Option<u32>,
    /// Name component following the parameter index and underscore.
    pub qualifier: Option<&'a str>,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Exact expression text following the serialized name separator.
    pub expression: &'a str,
    /// Finite value when the expression is context-free arithmetic.
    pub value: Option<f64>,
}

/// One validated external entity-index/object-id-table pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSection<'a> {
    /// Self-anchored base used by every entity-index offset.
    pub base: usize,
    /// Absolute offset of the entity-index array.
    pub entity_index_offset: usize,
    /// Absolute offset of the object-id table or offset-only identity metadata.
    pub object_id_table_offset: usize,
    /// Length-framed class definitions preceding the entity index.
    pub types: Vec<TypeDefinition<'a>>,
    /// Length-framed member definitions preceding the entity index.
    pub fields: Vec<FieldDefinition<'a>>,
    /// Store-level control block bounded by slot zero in an offset-only index.
    pub control: Option<EntityRecord<'a>>,
    /// Contiguous column-storage region after the control block.
    ///
    /// Present only for an offset-only store. Physical block boundaries do not
    /// delimit logical field lanes within this region.
    pub column_storage: Option<&'a [u8]>,
    /// Entity records following the reserved zero-offset slot.
    pub records: Vec<EntityRecord<'a>>,
}

/// One size-framed NX object-model section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Offset of the `ff ff ff ff` section signature.
    pub offset: usize,
    /// Complete section length including its 16-byte header.
    pub byte_len: usize,
    /// Class declarations in the section's contiguous type registry.
    pub types: Vec<TypeDefinition<'a>>,
    /// Member declarations in the section's field registry.
    pub fields: Vec<FieldDefinition<'a>>,
    /// Absolute offset of the section's internally pointed record area.
    pub record_area_offset: Option<usize>,
    /// Exact record-area bytes, including its 12-byte control prefix.
    pub record_area: Option<&'a [u8]>,
}

/// A feature operation name in a size-framed feature-history record area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationLabel<'a> {
    /// Absolute offset of the fixed operation-header marker.
    pub header_offset: usize,
    /// Absolute offset of the `03` label tag within the containing entry.
    pub offset: usize,
    /// Printable operation name without its terminating NUL.
    pub value: &'a str,
    /// Four object-index slots in header order; `None` is the `ff` sentinel.
    pub object_indices: [Option<u32>; 4],
    /// Absolute byte offset of each object-index token in header order.
    pub object_index_offsets: [usize; 4],
}

/// One operation record bounded by consecutive validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationRecord<'a> {
    /// Absolute offset of the fixed operation-header marker.
    pub offset: usize,
    /// Complete record bytes through the next operation header or section end.
    pub bytes: &'a [u8],
    /// Label decoded from this record's header.
    pub label: OperationLabel<'a>,
}

/// Primary body-object reference carried by one bounded operation record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationBodyReference {
    /// Absolute offset of the object-index token.
    pub offset: usize,
    /// Referenced body object index.
    pub object_index: u32,
}

/// Boolean operation kind stored after an operation label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOperationKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// One feature-history Boolean with object-index operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooleanOperation {
    /// Absolute offset of the operation label tag.
    pub offset: usize,
    /// Boolean operation kind.
    pub kind: BooleanOperationKind,
    /// Object index of the target body.
    pub target: u32,
    /// Ordered object indices of the tool bodies.
    pub tools: Vec<u32>,
}

impl<'a> IndexedSection<'a> {
    /// Return the section base used by its external record offsets.
    pub const fn base_offset(&self) -> usize {
        self.base
    }

    /// Decode explicit numeric-expression text within bounded entity records.
    pub fn numeric_expressions(&self) -> Vec<NumericExpression<'a>> {
        self.numeric_expression_records()
            .into_iter()
            .map(|(_, expression)| expression)
            .collect()
    }

    /// Decode expressions together with their owning record ordinal.
    pub fn numeric_expression_records(&self) -> Vec<(usize, NumericExpression<'a>)> {
        if !self.records.iter().any(|record| {
            record
                .bytes
                .windows(b"hostglobalvariables".len())
                .any(|window| window == b"hostglobalvariables")
        }) {
            return Vec::new();
        }
        self.records
            .iter()
            .enumerate()
            .filter_map(|(record_ordinal, record)| {
                numeric_expression_at(record.bytes, record.offset, record.object_id)
                    .map(|expression| (record_ordinal, expression))
            })
            .collect()
    }

    /// Decode every strictly framed printable string in each bounded record.
    pub fn string_values(&self) -> Vec<(usize, usize, Option<u32>, StringValue<'a>)> {
        self.records
            .iter()
            .enumerate()
            .flat_map(|(record_ordinal, record)| {
                string_values(record.bytes, record.offset)
                    .into_iter()
                    .enumerate()
                    .map(move |(value_ordinal, value)| {
                        (record_ordinal, value_ordinal, record.object_id, value)
                    })
            })
            .collect()
    }

    /// Decode tagged cross-record references from every bounded record.
    pub fn references(&self) -> Vec<(usize, usize, Option<u32>, ReferenceValue)> {
        self.records
            .iter()
            .enumerate()
            .flat_map(|(record_ordinal, record)| {
                let mut references = record_references(record.bytes, record.offset);
                references.extend(counted_record_references(
                    record.bytes,
                    record.offset,
                    self.records.len(),
                ));
                references.sort_by_key(|reference| reference.offset);
                references
                    .into_iter()
                    .enumerate()
                    .map(move |(reference_ordinal, reference)| {
                        (
                            record_ordinal,
                            reference_ordinal,
                            record.object_id,
                            reference,
                        )
                    })
            })
            .collect()
    }
}

impl<'a> Section<'a> {
    /// Decode the validated record-area control and product header.
    pub fn record_area_header(&self) -> Option<RecordAreaHeader<'a>> {
        let bytes = self.record_area?;
        let offset = self.record_area_offset?;
        let control_words = [u32_at(bytes, 0)?, u32_at(bytes, 4)?, u32_at(bytes, 8)?];
        let suffix = bytes.get(12..)?;
        is_product_record(suffix).then_some(())?;
        let length = usize::from(suffix[2]) - 2;
        Some(RecordAreaHeader {
            offset,
            control_words,
            product: StoreVersion {
                offset: offset + 12,
                value: std::str::from_utf8(&suffix[3..3 + length]).ok()?,
            },
        })
    }

    /// Decode strictly framed operation labels from the pointed record area.
    pub fn operation_labels(&self) -> Vec<OperationLabel<'a>> {
        let Some(bytes) = self.record_area else {
            return Vec::new();
        };
        let Some(base_offset) = self.record_area_offset else {
            return Vec::new();
        };
        operation_labels(bytes, base_offset)
    }

    /// Decode fully framed Boolean operations from the pointed record area.
    pub fn boolean_operations(&self) -> Vec<BooleanOperation> {
        let Some(bytes) = self.record_area else {
            return Vec::new();
        };
        let Some(base_offset) = self.record_area_offset else {
            return Vec::new();
        };
        boolean_operations(bytes, base_offset)
    }

    /// Bound operation records by consecutive validated operation headers.
    pub fn operation_records(&self) -> Vec<OperationRecord<'a>> {
        let Some(bytes) = self.record_area else {
            return Vec::new();
        };
        let Some(base_offset) = self.record_area_offset else {
            return Vec::new();
        };
        operation_records(bytes, base_offset)
    }

    /// Decode unambiguous primary body references from bounded operation records.
    pub fn operation_body_references(&self) -> Vec<(usize, OperationBodyReference)> {
        self.operation_records()
            .into_iter()
            .enumerate()
            .filter_map(|(ordinal, record)| {
                operation_body_reference(record).map(|reference| (ordinal, reference))
            })
            .collect()
    }
}

/// Decode complete feature-operation headers and their label frames.
pub fn operation_labels(bytes: &[u8], base_offset: usize) -> Vec<OperationLabel<'_>> {
    const HEADER: &[u8] = &[
        0x80, 0xcd, 0x01, 0x04, 0x01, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0xff, 0xff,
    ];
    let mut labels = Vec::new();
    for marker in bytes
        .windows(HEADER.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == HEADER).then_some(offset))
    {
        let mut at = marker + HEADER.len();
        let mut object_indices = [None; 4];
        let mut object_index_offsets = [0; 4];
        let mut valid = true;
        for (slot, offset) in object_indices
            .iter_mut()
            .zip(object_index_offsets.iter_mut())
        {
            *offset = base_offset + at;
            let Some((value, next)) = feature_object_index(bytes, at) else {
                valid = false;
                break;
            };
            *slot = value;
            at = next;
        }
        if !valid || bytes.get(at) != Some(&0x03) {
            continue;
        }
        let Some(length) = bytes.get(at + 1).copied().map(usize::from) else {
            continue;
        };
        if length < 3 {
            continue;
        }
        let Some(end) = at.checked_add(length) else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Some(name) = bytes.get(at + 2..end) else {
            continue;
        };
        if !name
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            continue;
        }
        let Ok(value) = std::str::from_utf8(name) else {
            continue;
        };
        labels.push(OperationLabel {
            header_offset: base_offset + marker,
            offset: base_offset + at,
            value,
            object_indices,
            object_index_offsets,
        });
    }
    labels
}

/// Bound every validated operation header through its successor or area end.
pub fn operation_records(bytes: &[u8], base_offset: usize) -> Vec<OperationRecord<'_>> {
    let labels = operation_labels(bytes, base_offset);
    labels
        .iter()
        .enumerate()
        .filter_map(|(ordinal, label)| {
            let start = label.header_offset.checked_sub(base_offset)?;
            let end = labels
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.header_offset - base_offset);
            Some(OperationRecord {
                offset: label.header_offset,
                bytes: bytes.get(start..end)?,
                label: *label,
            })
        })
        .collect()
}

/// Decode the unique `01 02 10 index ff` primary-body field in one operation.
pub fn operation_body_reference(record: OperationRecord<'_>) -> Option<OperationBodyReference> {
    let mut matches = Vec::new();
    for marker in record
        .bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x01, 0x02, 0x10]).then_some(offset))
    {
        let token = marker + 3;
        let (value, end) = feature_object_index(record.bytes, token)?;
        if record.bytes.get(end) == Some(&0xff) {
            matches.push(OperationBodyReference {
                offset: record.offset + token,
                object_index: value?,
            });
        }
    }
    let [reference] = matches.as_slice() else {
        return None;
    };
    Some(*reference)
}

fn feature_object_index(bytes: &[u8], at: usize) -> Option<(Option<u32>, usize)> {
    let prefix = *bytes.get(at)?;
    match prefix {
        0x00..=0x7f => Some((Some(u32::from(prefix)), at + 1)),
        0x80..=0x8f => Some((
            Some(u32::from(prefix - 0x80) * 256 + u32::from(*bytes.get(at + 1)?)),
            at + 2,
        )),
        0x90 => Some((
            Some(u32::from(u16::from_be_bytes([
                *bytes.get(at + 1)?,
                *bytes.get(at + 2)?,
            ]))),
            at + 3,
        )),
        0xff => Some((None, at + 1)),
        _ => None,
    }
}

/// Decode Boolean target and tool lists following complete operation labels.
pub fn boolean_operations(bytes: &[u8], base_offset: usize) -> Vec<BooleanOperation> {
    const BODY_HEADER: &[u8] = &[
        0x31, 0x00, 0x00, 0x01, 0x00, 0x14, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03,
        0x00, 0x00, 0xe0, 0x7f, 0xff, 0xff, 0xff, 0x01, 0x01,
    ];
    operation_labels(bytes, base_offset)
        .into_iter()
        .filter_map(|label| {
            let kind = match label.value {
                "UNITE" => BooleanOperationKind::Unite,
                "SUBTRACT" => BooleanOperationKind::Subtract,
                "INTERSECT" => BooleanOperationKind::Intersect,
                _ => return None,
            };
            let at = label.offset.checked_sub(base_offset)?;
            let label_end = at.checked_add(usize::from(*bytes.get(at + 1)?))? + 1;
            if bytes.get(label_end..label_end + BODY_HEADER.len()) != Some(BODY_HEADER) {
                return None;
            }
            let (targets, next) =
                counted_feature_object_indices(bytes, label_end + BODY_HEADER.len())?;
            if targets.len() != 1 || bytes.get(next) != Some(&0) {
                return None;
            }
            let (tools, end) = counted_feature_object_indices(bytes, next + 1)?;
            if tools.is_empty() || bytes.get(end) != Some(&0) {
                return None;
            }
            Some(BooleanOperation {
                offset: label.offset,
                kind,
                target: targets[0],
                tools,
            })
        })
        .collect()
}

fn counted_feature_object_indices(bytes: &[u8], at: usize) -> Option<(Vec<u32>, usize)> {
    if bytes.get(at) != Some(&0x01) {
        return None;
    }
    let count = usize::from(*bytes.get(at + 1)?).checked_sub(1)?;
    let mut cursor = at + 2;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let (value, next) = feature_object_index(bytes, cursor)?;
        values.push(value?);
        cursor = next;
    }
    Some((values, cursor))
}

/// Decode count-framed runs of same-section record references.
pub fn counted_record_references(
    bytes: &[u8],
    base_offset: usize,
    record_count: usize,
) -> Vec<ReferenceValue> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        if bytes[at] != 0x01 || bytes[at + 1] < 2 {
            at += 1;
            continue;
        }
        let count = usize::from(bytes[at + 1] - 1);
        let Some(end) = at.checked_add(2 + count * 3) else {
            at += 1;
            continue;
        };
        if end > bytes.len() || (0..count).any(|index| bytes[at + 2 + index * 3] != 0x90) {
            at += 1;
            continue;
        }
        let mut run = Vec::with_capacity(count);
        for index in 0..count {
            let token = at + 2 + index * 3;
            let value = u16::from_be_bytes([bytes[token + 1], bytes[token + 2]]);
            if usize::from(value) >= record_count {
                run.clear();
                break;
            }
            run.push(ReferenceValue {
                offset: base_offset + token,
                kind: ReferenceKind::RecordOrdinal16,
                value: u32::from(value),
            });
        }
        if run.is_empty() {
            at += 1;
        } else {
            references.extend(run);
            at = end;
        }
    }
    references
}

/// Decode self-identifying persistent handles plus context-gated tagged refs.
pub fn record_references(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let mut out = references(bytes, base_offset)
        .into_iter()
        .filter(|reference| reference.kind == ReferenceKind::PersistentHandle)
        .collect::<Vec<_>>();
    out.extend(
        dense_reference_suffix(bytes, base_offset)
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Tagged28),
    );
    out.sort_by_key(|reference| reference.offset);
    out
}

/// Decode tagged references wholly contained in `bytes`.
pub fn references(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == 0xe0 {
            if let Some(raw) = bytes
                .get(at + 1..at + 5)
                .and_then(|raw| raw.try_into().ok())
            {
                out.push(ReferenceValue {
                    offset: base_offset + at,
                    kind: ReferenceKind::PersistentHandle,
                    value: u32::from_be_bytes(raw),
                });
                at += 5;
                continue;
            }
        } else if bytes[at] & 0xf0 == 0xc0 {
            if let Some(raw) = bytes.get(at..at + 4).and_then(|raw| raw.try_into().ok()) {
                out.push(ReferenceValue {
                    offset: base_offset + at,
                    kind: ReferenceKind::Tagged28,
                    value: u32::from_be_bytes(raw) & 0x0fff_ffff,
                });
                at += 4;
                continue;
            }
        }
        at += 1;
    }
    out
}

/// Decode a dense tagged-reference suffix from one bounded OM record.
///
/// Sparse marker-shaped words can be ordinary per-class field data. A suffix
/// is a reference stream only when it contains at least eight persistent
/// handles and complete reference tokens cover at least 90% of its bytes.
pub fn dense_reference_suffix(bytes: &[u8], base_offset: usize) -> Vec<ReferenceValue> {
    let references = references(bytes, 0);
    for (index, first) in references.iter().enumerate() {
        let suffix = &references[index..];
        let persistent = suffix
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::PersistentHandle)
            .count();
        if persistent < 8 {
            continue;
        }
        let covered = suffix
            .iter()
            .map(|reference| match reference.kind {
                ReferenceKind::PersistentHandle => 5,
                ReferenceKind::Tagged28 => 4,
                ReferenceKind::RecordOrdinal16 => 3,
            })
            .sum::<usize>();
        let span = bytes.len().saturating_sub(first.offset);
        if covered * 10 >= span * 9 {
            return suffix
                .iter()
                .map(|reference| ReferenceValue {
                    offset: base_offset + reference.offset,
                    ..*reference
                })
                .collect();
        }
    }
    Vec::new()
}

/// Decode `66 32 03` printable-string values wholly contained in `bytes`.
pub fn string_values(bytes: &[u8], base_offset: usize) -> Vec<StringValue<'_>> {
    const MARKER: &[u8] = &[0x66, 0x32, 0x03];
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let declared = usize::from(*bytes.get(offset + 3)?);
            let text_len = declared.checked_sub(2)?;
            let start = offset.checked_add(4)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            (!raw.is_empty()
                && raw
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
                && bytes.get(end) == Some(&0))
            .then(|| StringValue {
                offset: base_offset + offset,
                value: std::str::from_utf8(raw).expect("invariant: printable ASCII is valid UTF-8"),
            })
        })
        .collect()
}

/// Decode every strictly length-framed numeric expression in an OM payload.
///
/// The `hostglobalvariables` marker identifies the owning table. Individual
/// records are self-framed as `handle, 04, length, text, 00`, so expression
/// decoding does not depend on an object-id table having the same cardinality
/// as an external entity-index array.
pub fn numeric_expressions(bytes: &[u8]) -> Vec<NumericExpression<'_>> {
    if !bytes
        .windows(b"hostglobalvariables".len())
        .any(|window| window == b"hostglobalvariables")
    {
        return Vec::new();
    }
    bytes
        .windows(b"(Number [".len())
        .enumerate()
        .filter(|(_, window)| *window == b"(Number [")
        .filter_map(|(offset, _)| {
            numeric_expression_at(
                &bytes[offset.saturating_sub(3)..],
                offset.saturating_sub(3),
                None,
            )
        })
        .collect()
}

/// Locate independently size-framed OM sections and their type registries.
pub fn sections(bytes: &[u8]) -> Vec<Section<'_>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 16 <= bytes.len() {
        let Some(relative) = bytes[at..]
            .windows(4)
            .position(|window| window == [0xff; 4])
        else {
            break;
        };
        let offset = at + relative;
        let Some(payload_len) = bytes
            .get(offset + 8..offset + 12)
            .and_then(|raw| raw.try_into().ok())
            .map(u32::from_be_bytes)
            .map(|value| value as usize)
        else {
            break;
        };
        let Some(end) = offset
            .checked_add(16)
            .and_then(|header_end| header_end.checked_add(payload_len))
        else {
            at = offset + 4;
            continue;
        };
        if bytes.get(offset + 12..offset + 14) != Some(b"OM") || end > bytes.len() {
            at = offset + 4;
            continue;
        }
        let types = type_definitions(bytes, offset + 16, end);
        let field_start = types.last().map_or(offset + 16, |definition| {
            definition.offset + definition.name.len() + 2
        });
        let fields = field_definitions(bytes, field_start, end);
        let schema_end = fields.last().map_or(field_start, |definition| {
            definition.offset + definition.name.len() + 2
        });
        let record_area_offset = section_record_area_offset(bytes, offset, schema_end, end);
        out.push(Section {
            offset,
            byte_len: end - offset,
            types,
            fields,
            record_area_offset,
            record_area: record_area_offset.map(|start| &bytes[start..end]),
        });
        at = end;
    }
    out
}

fn section_record_area_offset(
    bytes: &[u8],
    section_offset: usize,
    schema_end: usize,
    section_end: usize,
) -> Option<usize> {
    let search_end = schema_end.saturating_add(4096).min(section_end);
    let mut matches = (schema_end..search_end.saturating_sub(3)).filter_map(|at| {
        let relative = usize::try_from(u32_at(bytes, at)?).ok()?;
        let target = section_offset.checked_add(relative)?;
        (target >= at + 4 && target + 15 <= section_end).then_some(())?;
        is_product_record(bytes.get(target + 12..section_end)?).then_some(target)
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn is_product_record(bytes: &[u8]) -> bool {
    if !matches!(bytes.get(..2), Some([0x04 | 0x05, 0x01])) {
        return false;
    }
    let Some(length) = bytes
        .get(2)
        .copied()
        .map(usize::from)
        .and_then(|declared| declared.checked_sub(2))
    else {
        return false;
    };
    let Some(end) = 3usize.checked_add(length) else {
        return false;
    };
    bytes.get(3..end).is_some_and(|text| {
        text.starts_with(b"NX ")
            && text
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    }) && bytes.get(end) == Some(&0)
}

/// Locate validated NX OM entity-index/object-id-table pairs.
///
/// A candidate is accepted only when the arrays are adjacent, the index is
/// monotone, its first offset is zero, its second offset self-anchors the first
/// entity exactly at the end of the object-id table, and that entity carries the
/// NX root marker.
pub fn indexed_sections(bytes: &[u8]) -> Vec<IndexedSection<'_>> {
    let mut out = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    for table in 0..bytes.len().saturating_sub(4) {
        let Some(count) = u32_at(bytes, table).map(|value| value as usize) else {
            continue;
        };
        if !(2..=100_000).contains(&count) {
            continue;
        }
        let Some(index_len) = count.checked_add(1).and_then(|n| n.checked_mul(4)) else {
            continue;
        };
        let Some(index_start) = table.checked_sub(index_len) else {
            continue;
        };
        let Some(table_end) = count
            .checked_mul(4)
            .and_then(|length| table.checked_add(4 + length))
        else {
            continue;
        };
        if !is_root_record(bytes.get(table_end..).unwrap_or_default())
            || u32_at(bytes, index_start) != Some(0)
            || !seen_record_starts.insert(table_end)
        {
            continue;
        }
        let Some(first) = u32_at(bytes, index_start + 4).map(|value| value as usize) else {
            continue;
        };
        let Some(base) = table_end.checked_sub(first) else {
            continue;
        };
        let mut offsets = Vec::with_capacity(count + 1);
        for index in 0..=count {
            let Some(value) = u32_at(bytes, index_start + index * 4).map(|v| v as usize) else {
                offsets.clear();
                break;
            };
            offsets.push(value);
        }
        if offsets.len() != count + 1
            || offsets[1] == 0
            || !offsets.windows(2).all(|pair| pair[0] <= pair[1])
            || base
                .checked_add(offsets[count])
                .is_none_or(|end| end > bytes.len())
        {
            continue;
        }
        let mut records = Vec::with_capacity(count - 1);
        for index in 1..count {
            let start = base + offsets[index];
            let end = base + offsets[index + 1];
            let Some(payload) = bytes.get(start..end) else {
                records.clear();
                break;
            };
            let Some(object_id) = u32_at(bytes, table + 4 + index * 4) else {
                records.clear();
                break;
            };
            records.push(EntityRecord {
                object_id: Some(object_id),
                offset: start,
                bytes: payload,
            });
        }
        if records.len() == count - 1 {
            let types = type_definitions(bytes, base, index_start);
            let fields = all_field_definitions(bytes, base, index_start);
            out.push(IndexedSection {
                base,
                entity_index_offset: index_start,
                object_id_table_offset: table,
                types,
                fields,
                control: None,
                column_storage: None,
                records,
            });
        }
    }
    for count_offset in 8..bytes.len().saturating_sub(4) {
        let Some(record_count) = u32_at(bytes, count_offset).map(|value| value as usize) else {
            continue;
        };
        if !(2..=100_000).contains(&record_count) {
            continue;
        }
        let offset_count = record_count + 2;
        let Some(index_len) = offset_count.checked_mul(4) else {
            continue;
        };
        let Some(index_start) = count_offset.checked_sub(index_len) else {
            continue;
        };
        let Some(first) = u32_at(bytes, index_start).map(|value| value as usize) else {
            continue;
        };
        let Some(second) = u32_at(bytes, index_start + 4).map(|value| value as usize) else {
            continue;
        };
        let Some(last) = u32_at(bytes, count_offset - 4).map(|value| value as usize) else {
            continue;
        };
        if first < count_offset + 4 || first >= second || second > last || last > bytes.len() {
            continue;
        }
        let mut offsets = Vec::with_capacity(offset_count);
        for index in 0..offset_count {
            let Some(offset) = u32_at(bytes, index_start + index * 4).map(|v| v as usize) else {
                offsets.clear();
                break;
            };
            offsets.push(offset);
        }
        if offsets.len() != offset_count
            || offsets[0] < count_offset + 4
            || !offsets.windows(2).all(|pair| pair[0] <= pair[1])
            || offsets.last().is_none_or(|end| *end > bytes.len())
            || !seen_record_starts.insert(offsets[1])
        {
            continue;
        }
        let records = offsets[1..]
            .windows(2)
            .map(|bounds| EntityRecord {
                object_id: None,
                offset: bounds[0],
                bytes: &bytes[bounds[0]..bounds[1]],
            })
            .collect::<Vec<_>>();
        if records.len() != record_count {
            continue;
        }
        out.push(IndexedSection {
            base: 0,
            entity_index_offset: index_start,
            object_id_table_offset: offsets[0],
            types: type_definitions(bytes, 0, index_start),
            fields: all_field_definitions(bytes, 0, index_start),
            control: Some(EntityRecord {
                object_id: None,
                offset: offsets[0],
                bytes: &bytes[offsets[0]..offsets[1]],
            }),
            column_storage: Some(&bytes[offsets[1]..*offsets.last().expect("nonempty offsets")]),
            records,
        });
    }
    out
}

fn is_root_record(bytes: &[u8]) -> bool {
    if bytes.get(..2) != Some(&[0x04, 0x01]) {
        return false;
    }
    let Some(length) = bytes
        .get(2)
        .copied()
        .map(usize::from)
        .and_then(|declared| declared.checked_sub(2))
    else {
        return false;
    };
    let Some(end) = 3usize.checked_add(length) else {
        return false;
    };
    bytes.get(3..end).is_some_and(|text| {
        text.starts_with(b"NX ")
            && text
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    }) && bytes.get(end) == Some(&0)
}

/// Decode the first self-framed NX product/version marker in `bytes`.
pub fn store_version(bytes: &[u8], base_offset: usize) -> Option<StoreVersion<'_>> {
    (0..bytes.len().saturating_sub(3)).find_map(|at| {
        let suffix = &bytes[at..];
        is_root_record(suffix).then(|| {
            let length = usize::from(suffix[2]) - 2;
            StoreVersion {
                offset: base_offset + at,
                value: std::str::from_utf8(&suffix[3..3 + length])
                    .expect("validated printable NX version is UTF-8"),
            }
        })
    })
}

fn type_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<TypeDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        let declared = usize::from(bytes[at]);
        let Some(length) = declared.checked_sub(1) else {
            at += 1;
            continue;
        };
        let name_start = at + 1;
        let name_end = name_start.saturating_add(length);
        let Some(raw) = bytes.get(name_start..name_end) else {
            at += 1;
            continue;
        };
        let valid = raw.starts_with(b"UGS::")
            && raw.iter().all(|byte| (0x20..0x7f).contains(byte))
            && name_end < end;
        if valid {
            let name = std::str::from_utf8(raw)
                .expect("invariant: validated printable ASCII is valid UTF-8");
            out.push(TypeDefinition {
                offset: at,
                name,
                trailing_code: bytes[name_end],
                registry_suffix: &[],
            });
            at = name_end + 1;
        } else {
            at += 1;
        }
    }
    for index in 0..out.len().saturating_sub(1) {
        let suffix_start = out[index].offset + out[index].name.len() + 2;
        let suffix_end = out[index + 1].offset;
        out[index].registry_suffix = &bytes[suffix_start..suffix_end];
    }
    out
}

fn field_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut search = start;
    let mut limit = start.saturating_add(256).min(end);
    while let Some((definition, at)) = (search..limit)
        .find_map(|at| field_definition_at(bytes, at, end).map(|definition| (definition, at)))
    {
        let next = at + definition.name.len() + 2;
        search = next;
        limit = search.saturating_add(256).min(end);
        out.push(definition);
    }
    bound_field_registry_suffixes(bytes, &mut out);
    out
}

fn all_field_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        if let Some(definition) = field_definition_at(bytes, at, end) {
            at += definition.name.len() + 2;
            out.push(definition);
        } else {
            at += 1;
        }
    }
    bound_field_registry_suffixes(bytes, &mut out);
    out
}

fn bound_field_registry_suffixes<'a>(bytes: &'a [u8], definitions: &mut [FieldDefinition<'a>]) {
    for index in 0..definitions.len().saturating_sub(1) {
        let suffix_start = definitions[index].offset + definitions[index].name.len() + 2;
        let suffix_end = definitions[index + 1].offset;
        definitions[index].registry_suffix = &bytes[suffix_start..suffix_end];
    }
}

fn field_definition_at(bytes: &[u8], at: usize, end: usize) -> Option<FieldDefinition<'_>> {
    let declared = usize::from(*bytes.get(at)?);
    let length = declared.checked_sub(1)?;
    let name_start = at.checked_add(1)?;
    let name_end = name_start.checked_add(length)?;
    (name_end < end).then_some(())?;
    let raw = bytes.get(name_start..name_end)?;
    (raw.starts_with(b"m_") && raw.iter().all(|byte| (0x20..0x7f).contains(byte))).then_some(())?;
    Some(FieldDefinition {
        offset: at,
        name: std::str::from_utf8(raw).ok()?,
        trailing_code: bytes[name_end],
        registry_suffix: &[],
    })
}

fn numeric_expression_at(
    bytes: &[u8],
    base_offset: usize,
    object_id: Option<u32>,
) -> Option<NumericExpression<'_>> {
    const PREFIX: &[u8] = b"(Number [";
    let relative = bytes
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?;
    if relative < 3 || bytes.get(relative - 2) != Some(&0x04) {
        return None;
    }
    let declared = usize::from(*bytes.get(relative - 1)?);
    let text_len = declared.checked_sub(2)?;
    let text_end = relative.checked_add(text_len)?;
    (bytes.get(text_end) == Some(&0)).then_some(())?;
    let text = std::str::from_utf8(bytes.get(relative..text_end)?).ok()?;
    text.ends_with("; ").then_some(())?;
    let text = text.strip_prefix("(Number [")?;
    let (unit, rest) = text.split_once("]) ")?;
    let unit = match unit {
        "mm" => ExpressionUnit::Millimeter,
        "degrees" => ExpressionUnit::Degree,
        _ => return None,
    };
    let (name, value_tail) = rest.split_once(": ")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let value_text = value_tail.strip_suffix("; ")?;
    let (parameter_index, qualifier) = parameter_name(name);
    let value = evaluate_constant_expression(value_text);
    Some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name,
        parameter_index,
        qualifier,
        unit,
        expression: value_text,
        value,
    })
}

/// Evaluate the context-free arithmetic subset used by NX numeric formulas.
/// Names and function calls deliberately fail here; their values require the
/// owning parameter graph rather than local expression syntax.
pub(crate) fn evaluate_constant_expression(text: &str) -> Option<f64> {
    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
    }

    impl Parser<'_> {
        fn spaces(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
        }

        fn take(&mut self, byte: u8) -> bool {
            self.spaces();
            if self.bytes.get(self.at) == Some(&byte) {
                self.at += 1;
                true
            } else {
                false
            }
        }

        fn sum(&mut self) -> Option<f64> {
            let mut value = self.product()?;
            loop {
                if self.take(b'+') {
                    value += self.product()?;
                } else if self.take(b'-') {
                    value -= self.product()?;
                } else {
                    return value.is_finite().then_some(value);
                }
            }
        }

        fn product(&mut self) -> Option<f64> {
            let mut value = self.power()?;
            loop {
                if self.take(b'*') {
                    value *= self.power()?;
                } else if self.take(b'/') {
                    value /= self.power()?;
                } else {
                    return value.is_finite().then_some(value);
                }
            }
        }

        fn power(&mut self) -> Option<f64> {
            let value = self.unary()?;
            if self.take(b'^') {
                let result = value.powf(self.power()?);
                result.is_finite().then_some(result)
            } else {
                Some(value)
            }
        }

        fn unary(&mut self) -> Option<f64> {
            if self.take(b'+') {
                self.unary()
            } else if self.take(b'-') {
                Some(-self.unary()?)
            } else {
                self.primary()
            }
        }

        fn primary(&mut self) -> Option<f64> {
            if self.take(b'(') {
                let value = self.sum()?;
                self.take(b')').then_some(value)
            } else {
                self.number()
            }
        }

        fn number(&mut self) -> Option<f64> {
            self.spaces();
            let start = self.at;
            while self
                .bytes
                .get(self.at)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            {
                self.at += 1;
            }
            if self
                .bytes
                .get(self.at)
                .is_some_and(|byte| matches!(byte, b'e' | b'E'))
            {
                self.at += 1;
                if self
                    .bytes
                    .get(self.at)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    self.at += 1;
                }
                let exponent = self.at;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                (self.at > exponent).then_some(())?;
            }
            (self.at > start).then_some(())?;
            std::str::from_utf8(&self.bytes[start..self.at])
                .ok()?
                .parse()
                .ok()
        }
    }

    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.sum()?;
    parser.spaces();
    (parser.at == parser.bytes.len() && value.is_finite()).then_some(value)
}

fn parameter_name(name: &str) -> (Option<u32>, Option<&str>) {
    let Some(tail) = name.strip_prefix('p') else {
        return (None, None);
    };
    let digit_count = tail.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return (None, None);
    }
    let index = tail[..digit_count].parse().ok();
    let qualifier = tail
        .get(digit_count..)
        .and_then(|tail| tail.strip_prefix('_'))
        .filter(|qualifier| !qualifier.is_empty());
    (index, qualifier)
}
