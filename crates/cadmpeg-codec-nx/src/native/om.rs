// SPDX-License-Identifier: Apache-2.0
//! Object-model, data-block, expression, and external-reference extractors and record types.

#[allow(clippy::wildcard_imports)]
use super::*;
use crate::native::segments::segment_om_links;

/// Semantic family declared by a linked OM section's class registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmSchemaRole {
    /// General part object model declaring `UGS::Solid::Topol`.
    Model,
    /// Construction/history model declaring `UGS::FEATURE_RECORD`.
    FeatureHistory,
    /// Expression model declaring `UGS::EXP_expression`.
    Expressions,
    /// Audit model declaring `UGS::OM::SaveAuditTrail`.
    AuditTrail,
    /// No role marker from the defined families occurs in the registry.
    Other,
}

/// Internally pointed record area in a role-classified size-framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmRecordArea {
    /// Globally unique record-area identity.
    pub id: String,
    /// Link identifying the owning ordered OM section.
    pub section_link: String,
    /// Registry-derived role of the owning section.
    pub schema_role: OmSchemaRole,
    /// Three exact little-endian control words.
    pub control_words: [u32; 3],
    /// Exact printable product/version string.
    pub product_version: String,
    /// Exact record-area byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete pointed record area.
    pub sha256: String,
    /// Absolute file offset of the first control word.
    pub source_offset: u64,
}

/// Decode internally pointed record areas from linked OM sections.
pub fn om_record_areas(container: &Container) -> Vec<OmRecordArea> {
    let links = segment_om_links(container);
    let sections = container.om_sections();
    links
        .into_iter()
        .filter_map(|link| {
            let section = sections
                .iter()
                .find(|(entry, section)| {
                    entry
                        .file_span
                        .map_or(section.offset as u64, |(offset, _)| {
                            offset + section.offset as u64
                        })
                        == link.section_offset
                })?
                .1
                .clone();
            let header = section.record_area_header()?;
            let bytes = section.record_area?;
            let entry_offset = link.section_offset.checked_sub(section.offset as u64)?;
            let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(OmRecordArea {
                id: format!("nx:om-record-areas:area#{section_key}-{}", header.offset),
                section_link: link.id,
                schema_role: link.schema_role,
                control_words: header.control_words,
                product_version: header.product.value.to_string(),
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                source_offset: entry_offset + header.offset as u64,
            })
        })
        .collect()
}

/// Unit declared by an NX numeric expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as stored by NX.
    Degree,
}

/// Named parameter declaration in a bounded NX expression object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionDeclaration {
    /// Globally unique declaration identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: u32,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Exact NX parameter name.
    pub name: String,
    /// Decimal source parameter identifier following `p`.
    pub parameter_index: u32,
    /// Qualified role following the parameter identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    /// Directory entry containing the declaration record.
    pub source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    pub source_offset: u64,
}

/// Explicit numeric expression serialized in one NX OM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    /// Globally unique native-record identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: Option<u32>,
    /// Owning entry in the native OM record directory, when externally bounded.
    pub record: Option<String>,
    /// Exact-name declaration record for this parameter, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<String>,
    /// NX parameter name.
    pub name: String,
    /// Decimal source parameter identifier following the leading `p`.
    pub parameter_index: Option<u32>,
    /// Qualified role following the parameter identifier.
    pub qualifier: Option<String>,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Exact serialized expression text.
    #[allow(clippy::struct_field_names)]
    pub expression: String,
    /// Finite numeric value after context-free and dependency-graph evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Self-contained expression table selected by the nearest preceding table marker.
    #[serde(default)]
    pub source_table: String,
    /// Absolute file offset of the expression text.
    pub source_offset: u64,
}

/// Return exact `p<decimal>[_qualifier]` references in formula occurrence order.
pub(crate) fn expression_parameter_names(expression: &str) -> Vec<&str> {
    let bytes = expression.as_bytes();
    let mut names = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(end) = expression_parameter_reference_end(bytes, at) else {
            at += 1;
            continue;
        };
        names.push(&expression[at..end]);
        at = end;
    }
    names
}

fn expression_parameter_reference_end(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'p')
        || at
            .checked_sub(1)
            .and_then(|before| bytes.get(before))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }
    let mut end = at + 1;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    let name = std::str::from_utf8(bytes.get(at..end)?).ok()?;
    crate::om::parameter_name_parts(name).map(|_| end)
}

/// Length-framed class definition from an NX OM type registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Registered `UGS::` class name.
    pub name: String,
    /// Zero-based declaration ordinal used as class identity.
    pub ordinal: u32,
    /// Declaration code serialized after the class name.
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_prefix: Vec<u8>,
    /// Stable eight-byte class fingerprint in a framed registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_fingerprint: Option<[u8; 8]>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_terminal: Option<u8>,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the definition's length byte.
    pub source_offset: u64,
}

/// Member declaration from an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Globally unique declaration identity.
    pub id: String,
    /// Registered `m_` member name.
    pub name: String,
    /// Zero-based declaration ordinal within its section.
    pub ordinal: u32,
    /// Declaration code serialized immediately after the name.
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Absolute file offset of the containing OM section signature.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the declaration length byte.
    pub source_offset: u64,
}

/// Directory entry for one externally bounded NX OM entity record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Absolute file offset of the paired object-id table word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id_source_offset: Option<u64>,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based record ordinal within the indexed section.
    pub record_ordinal: u32,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    pub sha256: String,
    /// Ordered distinct same-section records referenced by this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Ordered distinct same-section records that reference this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the record start.
    pub source_offset: u64,
}

/// Counted active-object membership table from `RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmFastLoadObjectIdTable {
    /// Globally unique table identity.
    pub id: String,
    /// Ordered members in the native `rmfastload_object_ids` arena.
    pub members: Vec<String>,
    /// Exact serialized little-endian member-count word.
    pub raw_count: [u8; 4],
    /// Directory entry containing the table.
    pub source_entry: String,
    /// Absolute file offset of the `UGS::Solid::Topol` registry marker.
    pub registry_source_offset: u64,
    /// Absolute file offset of the four-byte count word.
    pub source_offset: u64,
}

/// One fixed-width active-object membership word from `RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmFastLoadObjectId {
    /// Globally unique member identity.
    pub id: String,
    /// Owning table in the native `rmfastload_object_id_tables` arena.
    pub table: String,
    /// Zero-based serialized member order.
    pub ordinal: u32,
    /// Decoded active object identifier.
    pub value: u32,
    /// Exact serialized little-endian object-id word.
    pub raw: [u8; 4],
    /// Absolute file offset of the four-byte object-id word.
    pub source_offset: u64,
}

/// One externally bounded block in an NX OM offset-only column store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlock {
    /// Globally unique block identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based block ordinal within the offset-only section.
    pub block_ordinal: u32,
    /// Whether this is the store control block or one data column block.
    pub role: DataBlockRole,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Exact serialized block length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized block bytes.
    pub sha256: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the block start.
    pub source_offset: u64,
}

/// Ordered value from a zero-prefixed offset-only OM store control array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlValue {
    /// Globally unique control-value identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based word order in the complete control block.
    pub ordinal: u32,
    /// Unsigned 24-bit value serialized after the zero byte.
    pub value: u32,
    /// Absolute file offset of the four-byte word.
    pub source_offset: u64,
}

/// Ordered little-endian value preceding a control-block product record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlIndexValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based value order in the aligned prefix array.
    pub ordinal: u32,
    /// Number of leading zero bytes before the aligned array.
    pub prefix_byte_len: u8,
    /// Unsigned little-endian value.
    pub value: u32,
    /// Same-section offset-store block addressed by an in-range value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_data_block: Option<String>,
    /// Absolute file offset of the four-byte value.
    pub source_offset: u64,
}

/// Registered class selected by the leading lane of an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlClassReference {
    /// Globally unique class-reference identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based order in the class-selection lane.
    pub ordinal: u32,
    /// Zero-based ordinal in the store's class registry.
    pub class_ordinal: u32,
    /// Target in the native `class_definitions` arena.
    pub class_definition: String,
    /// Exact registered class name.
    pub class_name: String,
    /// Absolute file offset of the four-byte control word.
    pub source_offset: u64,
}

/// Ordered object reference carried by an offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based reference order within the block.
    pub ordinal: u32,
    /// Referenced persistent OM object ID.
    pub object_id: u32,
    /// Exact serialized object-index token.
    pub raw_object_id: Vec<u8>,
    /// Uniquely resolved object record in the same directory entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_record: Option<String>,
    /// Uniquely resolved parameter declaration carrying this object ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_expression_declaration: Option<String>,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Complete counted block-index lane carried by one offset-store block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockCountedIndexLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based lane order within the block.
    pub ordinal: u32,
    /// Serialized count including the anchor and terminal slot.
    pub declared_count: u8,
    /// Decoded anchoring block index.
    pub anchor_index: u32,
    /// Exact serialized anchor token.
    pub raw_anchor_index: Vec<u8>,
    /// Same-section block addressed by the anchor.
    pub anchor_data_block: String,
    /// Ordered decoded member block indices.
    pub member_indices: Vec<u32>,
    /// Exact serialized member tokens in lane order.
    pub raw_member_indices: Vec<Vec<u8>>,
    /// Ordered same-section blocks addressed by the members.
    pub member_data_blocks: Vec<String>,
    /// Absolute file offset of the opening `01` marker.
    pub source_offset: u64,
    /// Absolute file offset of the anchoring compact index.
    pub anchor_source_offset: u64,
    /// Ordered absolute file offsets of member compact indices.
    pub member_source_offsets: Vec<u64>,
}

/// Fixed-width nullable `ABR` block-reference lane in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockAbrReferenceLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based lane order within the section's column storage.
    pub ordinal: u32,
    /// Sixteen ordered nullable serialized block indices.
    pub slot_indices: Vec<Option<u32>>,
    /// Exact compact-index tokens in slot order.
    pub raw_slot_indices: Vec<Vec<u8>>,
    /// Sixteen ordered nullable same-section block identities.
    pub slot_data_blocks: Vec<Option<String>>,
    /// Absolute file offsets of the sixteen compact-index tokens.
    pub slot_source_offsets: Vec<u64>,
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Absolute file offset of the opening `11` marker.
    pub source_offset: u64,
}

/// Self-framed index row in contiguous offset-store column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// First non-null compact index.
    pub first_index: u32,
    /// Exact serialized leading-index token.
    pub raw_first_index: Vec<u8>,
    /// Serialized `03` or `07` row flag.
    pub flag: u8,
    /// Four ordered non-null compact indices after the row flag.
    pub indices: [u32; 4],
    /// Exact serialized four-index tokens in row order.
    pub raw_indices: [Vec<u8>; 4],
    /// Four same-section blocks addressed by the compact indices.
    pub data_blocks: [String; 4],
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the first compact index.
    pub first_index_source_offset: u64,
    /// Four ordered absolute file offsets of the compact indices.
    pub index_source_offsets: [u64; 4],
}

/// Self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockLinkedIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Unresolved leading compact index.
    pub first_index: u32,
    /// Exact serialized leading-index token.
    pub raw_first_index: Vec<u8>,
    /// Serialized `16`, `17`, or `18` discriminator.
    pub discriminator: u8,
    /// Target compact block index.
    pub target_index: u32,
    /// Exact serialized target-index token.
    pub raw_target_index: Vec<u8>,
    /// Three compact block indices after `ff ff 90 fe`.
    pub indices: [u32; 3],
    /// Exact serialized post-marker tokens in row order.
    pub raw_indices: [Vec<u8>; 3],
    /// Target block followed by the three post-marker blocks.
    pub data_blocks: [String; 4],
    /// Serialized `03` or `07` flag.
    pub flag: u8,
    /// Serialized `04` or `07` mode.
    pub mode: u8,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the leading compact index.
    pub first_index_source_offset: u64,
    /// Absolute file offset of the target compact index.
    pub target_index_source_offset: u64,
    /// Absolute file offsets of the three post-marker indices.
    pub index_source_offsets: [u64; 3],
}

/// Self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockTargetIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Target compact block index.
    pub target_index: u32,
    /// Exact serialized target-index token.
    pub raw_target_index: Vec<u8>,
    /// Three compact block indices after `ff ff 90 fe`.
    pub indices: [u32; 3],
    /// Exact serialized post-marker tokens in row order.
    pub raw_indices: [Vec<u8>; 3],
    /// Target block followed by the three post-marker blocks.
    pub data_blocks: [String; 4],
    /// Serialized `04` or `07` mode.
    pub mode: u8,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the target compact index.
    pub target_index_source_offset: u64,
    /// Absolute file offsets of the three post-marker indices.
    pub index_source_offsets: [u64; 3],
}

/// Complete composite table spanning linked and target-index row grammars.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockColumnIndexTable {
    /// Globally unique table identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Leading mode-7 linked row.
    pub opening_linked_row: String,
    /// Consecutive target-index rows in ascending source order.
    pub target_rows: Vec<String>,
    /// Consecutive mode-4 linked rows in ascending source order.
    pub linked_rows: Vec<String>,
    /// First and greatest target block ordinal.
    pub first_target_index: u32,
    /// Last and least target block ordinal.
    pub last_target_index: u32,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Absolute source offset of the opening linked row.
    pub source_offset: u64,
}

/// Product/version header from one indexed NX OM store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreHeader {
    /// Globally unique store-header identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Persistent object identity when the header belongs to an ID-bounded record.
    pub object_id: Option<u32>,
    /// Exact printable product/version text.
    pub version: String,
    /// Directory entry containing the OM store.
    pub source_entry: String,
    /// Absolute file offset of the `04 01` marker.
    pub source_offset: u64,
}

/// Role of one bounded block in an offset-only NX OM store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBlockRole {
    /// Store-level schema and root metadata from boundary slot zero.
    Control,
    /// One offset-bounded column-storage block.
    Column,
}

/// Self-framed printable string carried by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `66 32 03` marker.
    pub source_offset: u64,
}

/// Tagged reference family serialized in an NX OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectReferenceKind {
    /// `e0` marker followed by a 32-bit persistent handle.
    PersistentHandle,
    /// Four-byte `0xC?` tagged 28-bit reference.
    Tagged28,
    /// Count-framed `90` reference to a record ordinal in the same section.
    RecordOrdinal16,
}

/// Ordered tagged-reference occurrence owned by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker/tag bits.
    pub value: u32,
    /// Resolved target in the native OM record directory.
    pub target_record: Option<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Ordered persistent or tagged reference in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based retained-reference order within the control block.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker or tag bits.
    pub value: u32,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Exact two-token persistent-handle run in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlHandlePair {
    /// Globally unique pair identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// First handle-reference occurrence.
    pub first_reference: String,
    /// Second handle-reference occurrence.
    pub second_reference: String,
    /// First persistent-handle value.
    pub first_handle: u32,
    /// Second persistent-handle value.
    pub second_handle: u32,
    /// Absolute file offset of the first `e0` marker.
    pub source_offset: u64,
}

/// Cross-record identity established by equal persistent-handle values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentHandle {
    /// Globally unique handle identity.
    pub id: String,
    /// Unsigned persistent-handle value.
    pub value: u32,
    /// Ordered distinct OM directory records containing the handle.
    pub records: Vec<String>,
    /// Total serialized occurrences across OM records and offset-store control blocks.
    pub occurrence_count: u32,
    /// Ordered distinct offset-store control blocks containing the handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_blocks: Vec<String>,
    /// Ordered distinct EXTREFSTREAM records containing the same handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_records: Vec<String>,
    /// Total serialized occurrences across EXTREFSTREAM record prefixes and tails.
    #[serde(default)]
    pub external_occurrence_count: u32,
}

/// Named NX arrangement from `/Root/part/arrangements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration {
    /// Globally unique native-record identity.
    pub id: String,
    /// Arrangement name.
    pub name: String,
    /// Whether NX marks this arrangement as the default.
    pub is_default: bool,
    /// Directory entry containing the arrangement XML.
    pub source_entry: String,
    /// Absolute file offset of the arrangement element.
    pub source_offset: u64,
}

/// Exact agreement between the default arrangement and the part attribute
/// naming the active arrangement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationAttributeUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Default arrangement from the native configuration arena.
    pub configuration: String,
    /// Typed `NX_Arrangement` part attribute carrying the same name.
    pub part_attribute: String,
    /// Exact shared arrangement name.
    pub name: String,
}

/// One typed part-level attribute from `/Root/part/attrs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartAttribute {
    /// Globally unique native-record identity.
    pub id: String,
    /// Attribute owner token.
    pub owner: String,
    /// UTF-8 attribute title.
    pub title: String,
    /// UTF-8 attribute value.
    pub value: String,
    /// XML schema type token.
    pub value_type: String,
    /// Whether product-data management owns the value.
    pub pdm_based: bool,
    /// Attribute record schema version.
    pub version: u32,
    /// Directory entry containing the attribute XML.
    pub source_entry: String,
    /// Absolute file offset of the attribute element.
    pub source_offset: u64,
}

/// End-anchored child-part string from an NX external-reference stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReference {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based string-table ordinal within the stream.
    pub ordinal: u32,
    /// Exact serialized child-part name or path.
    pub path: String,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the first path byte.
    pub source_offset: u64,
}

/// Externally bounded record retained from an EXTREFSTREAM index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceIndexedRecord {
    /// Globally unique indexed-record identity.
    pub id: String,
    /// Record type from the external-reference directory.
    pub record_id: u32,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    pub sha256: String,
    /// Specialized handle-set record when that complete grammar resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_set_record: Option<String>,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the indexed record.
    pub source_offset: u64,
}

/// Indexed EXTREFSTREAM record prefix with its exact handle membership set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Record type from the external-reference directory.
    pub record_id: u32,
    /// Count declared before the four ID slots.
    pub declared_count: u16,
    /// Four uninterpreted little-endian ID slots.
    pub id_slots: [u32; 4],
    /// Strictly ascending persistent handles; the serialized closing duplicate is omitted.
    pub handles: Vec<u32>,
    /// Whether the final serialized handle repeats the preceding handle.
    pub closing_duplicate: bool,
    /// Length of the decoded record prefix.
    pub prefix_byte_len: u64,
    /// Length after the decoded handle-set prefix and before the next record or string table.
    pub tail_byte_len: u64,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the record marker.
    pub source_offset: u64,
}

/// Empty EXTREFSTREAM indexed-record form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceEmptyRecord {
    /// Globally unique empty-record identity.
    pub id: String,
    /// Owning record in the native `external_reference_indexed_records` arena.
    pub indexed_record: String,
    /// Whether the six-byte header is followed by a closing `01` marker.
    pub closing_marker: bool,
}

/// Exact adjacent reference pair in an EXTREFSTREAM handle-set tail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceTailReferencePair {
    /// Globally unique pair identity.
    pub id: String,
    /// Owning record in the native `external_reference_records` arena.
    pub handle_set_record: String,
    /// Zero-based pair order within the bounded tail.
    pub ordinal: u32,
    /// Persistent handle from the `e0 + u32 BE` token.
    pub persistent_handle: u32,
    /// Low 28 bits of the following four-byte `0xC?` reference.
    pub tagged_reference: u32,
    /// Absolute file offset of the `e0` marker.
    pub source_offset: u64,
}

/// One external-reference record slot resolved through its same-stream string table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecordStringUse {
    /// Globally unique slot-use identity.
    pub id: String,
    /// Owning record in the native `external_reference_records` arena.
    pub external_record: String,
    /// Zero-based slot in the record's four-value lane.
    pub slot: u8,
    /// Serialized string-table index.
    pub string_index: u32,
    /// Target in the native `external_references` arena.
    pub external_reference: String,
    /// Absolute file offset of the serialized `u32 LE` slot value.
    pub source_offset: u64,
}

/// Child-part identity selected by one complete external-reference record lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecordChild {
    /// Globally unique child-binding identity.
    pub id: String,
    /// Owning record in the native `external_reference_records` arena.
    pub external_record: String,
    /// Slot-zero child filename in the native `external_references` arena.
    pub name_reference: String,
    /// Slot-two child directory in the native `external_references` arena.
    pub directory_reference: String,
}

/// Embedded NX material texture stored as a TIFF stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialTextureAsset {
    /// Globally unique native-record identity.
    pub id: String,
    /// Texture stream leaf name carried by the directory path.
    pub name: String,
    /// TIFF byte order: `little_endian` or `big_endian`.
    pub byte_order: String,
    /// TIFF format version. NX material textures use version 42.
    pub version: u16,
    /// Absolute byte offset of the first TIFF image-file directory, relative to the asset payload.
    pub first_ifd_offset: u32,
    /// Exact texture payload length.
    pub byte_len: u64,
    /// SHA-256 digest of the exact TIFF payload.
    pub sha256: String,
    /// Directory entry containing the texture.
    pub source_entry: String,
    /// Absolute file offset of the TIFF header.
    pub source_offset: u64,
}

/// Exact QAF catalog mapping for one embedded material texture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialTextureCatalogEntry {
    /// Globally unique native relation identity.
    pub id: String,
    /// Target in the native `material_texture_assets` arena.
    pub texture_asset: String,
    /// Stored path relative to `/Root/`.
    pub storage_path: String,
    /// Logical material-texture path recorded by QAF metadata.
    pub material_path: String,
    /// Exact QAF creation-time text.
    pub create_time: String,
    /// Exact QAF modification-time text.
    pub modify_time: String,
    /// Directory entry containing the QAF catalog.
    pub source_entry: String,
    /// Absolute file offset of the `folderProperties` element.
    pub source_offset: u64,
}

/// Decode every strictly framed TIFF material-texture directory entry.
pub fn material_texture_assets(container: &Container) -> Vec<MaterialTextureAsset> {
    let mut assets = container
        .entries
        .iter()
        .filter_map(|entry| {
            let name = entry.name.strip_prefix("/Root/materialsTif/")?;
            (!name.is_empty()).then_some(())?;
            let (offset, size) = entry.file_span?;
            let (start, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container.data.get(start..start.checked_add(size)?)?;
            let (byte_order, version, first_ifd_offset) = match payload.get(..8)? {
                [b'I', b'I', 42, 0, a, b, c, d] => {
                    ("little_endian", 42, u32::from_le_bytes([*a, *b, *c, *d]))
                }
                [b'M', b'M', 0, 42, a, b, c, d] => {
                    ("big_endian", 42, u32::from_be_bytes([*a, *b, *c, *d]))
                }
                _ => return None,
            };
            let first_ifd = usize::try_from(first_ifd_offset).ok()?;
            (first_ifd >= 8 && first_ifd < payload.len()).then_some(())?;
            Some(MaterialTextureAsset {
                id: String::new(),
                name: name.to_string(),
                byte_order: byte_order.to_string(),
                version,
                first_ifd_offset,
                byte_len: size as u64,
                sha256: sha256_hex(payload),
                source_entry: entry.name.clone(),
                source_offset: offset,
            })
        })
        .collect::<Vec<_>>();
    assets.sort_by(|first, second| first.source_entry.cmp(&second.source_entry));
    for (ordinal, asset) in assets.iter_mut().enumerate() {
        asset.id = format!("nx:container:material-texture#{ordinal}");
    }
    assets
}

/// Join QAF material paths to embedded TIFF streams by exact stored path.
pub fn material_texture_catalog_entries(
    container: &Container,
    assets: &[MaterialTextureAsset],
) -> Vec<MaterialTextureCatalogEntry> {
    let Some((entry_index, entry)) = container
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == "/Root/qafmetadata")
    else {
        return Vec::new();
    };
    let Some((entry_offset, size)) = entry.file_span else {
        return Vec::new();
    };
    let Some(start) = usize::try_from(entry_offset).ok() else {
        return Vec::new();
    };
    let Some(size) = usize::try_from(size).ok() else {
        return Vec::new();
    };
    let Some(end) = start.checked_add(size) else {
        return Vec::new();
    };
    let Some(payload) = container.data.get(start..end) else {
        return Vec::new();
    };
    let Some(entries) =
        parse_material_texture_catalog(payload, entry_index, &entry.name, entry_offset, assets)
    else {
        return Vec::new();
    };
    entries
}

fn parse_material_texture_catalog(
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
    assets: &[MaterialTextureAsset],
) -> Option<Vec<MaterialTextureCatalogEntry>> {
    let document = roxmltree::Document::parse(xml_stream_text(payload)?).ok()?;
    let root = document.root_element();
    (root.tag_name().name() == "folderContents").then_some(())?;
    let assets_by_path = assets
        .iter()
        .map(|asset| Some((asset.source_entry.strip_prefix("/Root/")?, asset)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let mut catalog = Vec::new();
    let mut seen_assets = BTreeSet::new();
    for node in root.children().filter(roxmltree::Node::is_element) {
        (node.tag_name().name() == "folderProperties").then_some(())?;
        let storage_path = node.attribute("location")?;
        let material_path = node.attribute("unmappedLocation")?;
        let children = node
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let [create, modify] = children.as_slice() else {
            return None;
        };
        (create.tag_name().name() == "createTime" && modify.tag_name().name() == "modifyTime")
            .then_some(())?;
        let create_time = create.text()?;
        let modify_time = modify.text()?;
        if !storage_path.starts_with("materialsTif/") {
            continue;
        }
        let asset = assets_by_path.get(storage_path)?;
        material_path
            .strip_prefix("materialsTif/")
            .filter(|name| !name.is_empty())?;
        seen_assets.insert(asset.id.as_str()).then_some(())?;
        let ordinal = catalog.len();
        catalog.push(MaterialTextureCatalogEntry {
            id: format!("nx:qafmetadata-{entry_index}:material-texture#{ordinal}"),
            texture_asset: asset.id.clone(),
            storage_path: storage_path.to_string(),
            material_path: material_path.to_string(),
            create_time: create_time.to_string(),
            modify_time: modify_time.to_string(),
            source_entry: source_entry.to_string(),
            source_offset: entry_offset + node.range().start as u64,
        });
    }
    Some(catalog)
}

/// Decode end-anchored external child-part string tables.
pub fn external_references(container: &Container) -> Vec<ExternalReference> {
    let mut ordinals = BTreeMap::<String, u32>::new();
    container
        .external_reference_strings()
        .into_iter()
        .map(|(entry, relative, path)| {
            let ordinal = ordinals.entry(entry.name.clone()).or_default();
            let current = *ordinal;
            *ordinal += 1;
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            ExternalReference {
                id: format!("nx:external-reference:{}#{current}", entry.name),
                ordinal: current,
                path,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + relative as u64,
            }
        })
        .collect()
}

/// Decode exact indexed external-reference record prefixes.
pub fn external_reference_records(container: &Container) -> Vec<ExternalReferenceRecord> {
    container
        .external_reference_records()
        .into_iter()
        .map(|(entry, record)| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            ExternalReferenceRecord {
                id: format!(
                    "nx:external-reference-record:{}#{}",
                    entry.name, record.record_id
                ),
                record_id: record.record_id,
                declared_count: record.declared_count,
                id_slots: record.id_slots,
                handles: record.handles,
                closing_duplicate: record.closing_duplicate,
                prefix_byte_len: record.prefix_byte_len as u64,
                tail_byte_len: record.tail_byte_len as u64,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + record.offset as u64,
            }
        })
        .collect()
}

/// Retain all indexed records and link uniquely decoded handle-set records.
pub fn external_reference_indexed_records(
    container: &Container,
    decoded: &[ExternalReferenceRecord],
) -> Vec<ExternalReferenceIndexedRecord> {
    let mut decoded_by_key = BTreeMap::<(&str, u32), Option<&ExternalReferenceRecord>>::new();
    for record in decoded {
        decoded_by_key
            .entry((record.source_entry.as_str(), record.record_id))
            .and_modify(|value| *value = None)
            .or_insert(Some(record));
    }
    container
        .external_reference_indexed_records()
        .into_iter()
        .filter_map(|(entry, record)| {
            let entry_offset = entry.file_span?.0;
            let source_offset = entry_offset.checked_add(record.offset as u64)?;
            let start = usize::try_from(source_offset).ok()?;
            let bytes = container
                .data
                .get(start..start.checked_add(record.byte_len)?)?;
            Some(ExternalReferenceIndexedRecord {
                id: format!(
                    "nx:external-reference-indexed-record:{}#{}",
                    entry.name, record.record_id
                ),
                record_id: record.record_id,
                byte_len: record.byte_len as u64,
                sha256: sha256_hex(bytes),
                handle_set_record: decoded_by_key
                    .get(&(entry.name.as_str(), record.record_id))
                    .and_then(|record| *record)
                    .map(|record| record.id.clone()),
                source_entry: entry.name.clone(),
                source_offset,
            })
        })
        .collect()
}

/// Decode every exact six- or seven-byte empty indexed record.
pub fn external_reference_empty_records(
    container: &Container,
    indexed: &[ExternalReferenceIndexedRecord],
) -> Vec<ExternalReferenceEmptyRecord> {
    indexed
        .iter()
        .filter_map(|record| {
            let start = usize::try_from(record.source_offset).ok()?;
            let length = usize::try_from(record.byte_len).ok()?;
            let bytes = container.data.get(start..start.checked_add(length)?)?;
            let closing_marker = crate::container::parse_extref_empty_record(bytes)?;
            Some(ExternalReferenceEmptyRecord {
                id: record.id.replacen("indexed-record", "empty-record", 1),
                indexed_record: record.id.clone(),
                closing_marker,
            })
        })
        .collect()
}

/// Decode exact adjacent reference pairs from bounded handle-set tails.
pub fn external_reference_tail_reference_pairs(
    container: &Container,
    records: &[ExternalReferenceRecord],
) -> Vec<ExternalReferenceTailReferencePair> {
    records
        .iter()
        .flat_map(|record| {
            let Some(start) = usize::try_from(record.source_offset)
                .ok()
                .and_then(|start| start.checked_add(usize::try_from(record.prefix_byte_len).ok()?))
            else {
                return Vec::new();
            };
            let Some(length) = usize::try_from(record.tail_byte_len).ok() else {
                return Vec::new();
            };
            let Some(end) = start.checked_add(length) else {
                return Vec::new();
            };
            let Some(bytes) = container.data.get(start..end) else {
                return Vec::new();
            };
            crate::container::parse_extref_reference_pairs(bytes)
                .into_iter()
                .enumerate()
                .map(|(ordinal, (offset, persistent_handle, tagged_reference))| {
                    let record_key = record
                        .id
                        .split_once('#')
                        .map_or(record.id.as_str(), |(_, key)| key);
                    ExternalReferenceTailReferencePair {
                        id: format!(
                            "nx:external-reference:tail-reference-pair#{record_key}-{ordinal}"
                        ),
                        handle_set_record: record.id.clone(),
                        ordinal: ordinal as u32,
                        persistent_handle,
                        tagged_reference,
                        source_offset: (start + offset) as u64,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resolve complete four-slot record lanes through same-stream string tables.
pub fn external_reference_record_string_uses(
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
) -> Vec<ExternalReferenceRecordStringUse> {
    let mut references_by_key = BTreeMap::<(&str, u32), Option<&ExternalReference>>::new();
    for reference in references {
        references_by_key
            .entry((reference.source_entry.as_str(), reference.ordinal))
            .and_modify(|value| *value = None)
            .or_insert(Some(reference));
    }
    records
        .iter()
        .flat_map(|record| {
            if record.source_offset.checked_add(19).is_none() {
                return Vec::new();
            }
            let resolved = record
                .id_slots
                .iter()
                .map(|index| {
                    references_by_key
                        .get(&(record.source_entry.as_str(), *index))
                        .and_then(|reference| *reference)
                })
                .collect::<Option<Vec<_>>>();
            let Some(resolved) = resolved else {
                return Vec::new();
            };
            resolved
                .into_iter()
                .enumerate()
                .map(|(slot, reference)| {
                    let record_key = record
                        .id
                        .split_once('#')
                        .map_or(record.id.as_str(), |(_, key)| key);
                    ExternalReferenceRecordStringUse {
                        id: format!("nx:external-reference:record-string-use#{record_key}-{slot}"),
                        external_record: record.id.clone(),
                        slot: slot as u8,
                        string_index: record.id_slots[slot],
                        external_reference: reference.id.clone(),
                        source_offset: record.source_offset + 7 + slot as u64 * 4,
                    }
                })
                .collect()
        })
        .collect()
}

/// Bind complete record lanes to their slot-zero name and slot-two directory.
pub fn external_reference_record_children(
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
    uses: &[ExternalReferenceRecordStringUse],
) -> Vec<ExternalReferenceRecordChild> {
    let mut references_by_id = BTreeMap::<&str, Option<&ExternalReference>>::new();
    for reference in references {
        references_by_id
            .entry(reference.id.as_str())
            .and_modify(|value| *value = None)
            .or_insert(Some(reference));
    }
    records
        .iter()
        .filter_map(|record| {
            let mut record_uses = uses
                .iter()
                .filter(|use_| use_.external_record == record.id)
                .collect::<Vec<_>>();
            record_uses.sort_by_key(|use_| use_.slot);
            let [slot0, slot1, slot2, slot3] = record_uses.as_slice() else {
                return None;
            };
            if [slot0.slot, slot1.slot, slot2.slot, slot3.slot] != [0, 1, 2, 3] {
                return None;
            }
            let resolved = record_uses
                .iter()
                .enumerate()
                .map(|(slot, use_)| {
                    let reference = references_by_id
                        .get(use_.external_reference.as_str())
                        .and_then(|reference| *reference)?;
                    (use_.string_index == record.id_slots[slot]
                        && reference.source_entry == record.source_entry
                        && reference.ordinal == use_.string_index)
                        .then_some(reference)
                })
                .collect::<Option<Vec<_>>>()?;
            let name = resolved[0];
            let directory = resolved[2];
            name.path
                .to_ascii_lowercase()
                .ends_with(".prt")
                .then_some(())?;
            (!directory.path.is_empty()).then_some(())?;
            Some(ExternalReferenceRecordChild {
                id: format!("{}:child", record.id),
                external_record: record.id.clone(),
                name_reference: name.id.clone(),
                directory_reference: directory.id.clone(),
            })
        })
        .collect()
}

/// Decode the explicit NX arrangement table.
pub fn configurations(container: &Container) -> Vec<Configuration> {
    if container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/part/arrangements")
        .count()
        != 1
    {
        return Vec::new();
    }
    container
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.name == "/Root/part/arrangements")
        .filter_map(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let (offset_usize, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container
                .data
                .get(offset_usize..offset_usize.checked_add(size)?)?;
            let xml = xml_stream_text(payload)?;
            let document = roxmltree::Document::parse(xml).ok()?;
            let root = document.root_element();
            if root.tag_name().name() != "Arrangements" {
                return None;
            }

            let mut active_count = 0usize;
            let mut names = BTreeSet::new();
            let mut configurations = Vec::new();
            for (ordinal, node) in root
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                if node.tag_name().name() != "Arrangement" {
                    return None;
                }
                let name = node.attribute("Name")?;
                if name.is_empty() || !names.insert(name) {
                    return None;
                }
                let is_default = match node.attribute("Default")? {
                    "YES" => true,
                    "NO" => false,
                    _ => return None,
                };
                active_count += usize::from(is_default);
                configurations.push(Configuration {
                    id: format!("nx:arrangements-{entry_index}:configuration#{ordinal}"),
                    name: name.to_string(),
                    is_default,
                    source_entry: entry.name.clone(),
                    source_offset: offset + node.range().start as u64,
                });
            }
            (!configurations.is_empty() && active_count <= 1).then_some(configurations)
        })
        .flatten()
        .collect()
}

/// Join the two independently framed active-arrangement declarations.
pub fn configuration_attribute_uses(
    configurations: &[Configuration],
    attributes: &[PartAttribute],
) -> Vec<ConfigurationAttributeUse> {
    let active = configurations
        .iter()
        .filter(|configuration| configuration.is_default)
        .collect::<Vec<_>>();
    let declarations = attributes
        .iter()
        .filter(|attribute| {
            attribute.owner == "part"
                && attribute.title == "NX_Arrangement"
                && attribute.value_type == "StringAttributeType"
        })
        .collect::<Vec<_>>();
    let ([configuration], [attribute]) = (active.as_slice(), declarations.as_slice()) else {
        return Vec::new();
    };
    if configuration.name != attribute.value {
        return Vec::new();
    }
    vec![ConfigurationAttributeUse {
        id: "nx:arrangements:active-attribute-use#0".to_string(),
        configuration: configuration.id.clone(),
        part_attribute: attribute.id.clone(),
        name: configuration.name.clone(),
    }]
}

/// Decode the typed part-attribute XML stream atomically.
pub fn part_attributes(container: &Container) -> Vec<PartAttribute> {
    if container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/part/attrs")
        .count()
        != 1
    {
        return Vec::new();
    }
    container
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == "/Root/part/attrs")
        .and_then(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let start = usize::try_from(offset).ok()?;
            let payload = container
                .data
                .get(start..start.checked_add(usize::try_from(size).ok()?)?)?;
            parse_part_attributes(payload, entry_index, &entry.name, offset)
        })
        .unwrap_or_default()
}

pub(crate) fn parse_part_attributes(
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
) -> Option<Vec<PartAttribute>> {
    let document = roxmltree::Document::parse(xml_stream_text(payload)?).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "UgAttributes"
        || root.attribute("version")?.parse::<u32>().ok()? < 4
    {
        return None;
    }
    root.children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
        .map(|(ordinal, node)| {
            if node.tag_name().name() != "Attribute" {
                return None;
            }
            Some(PartAttribute {
                id: format!("nx:part-attributes-{entry_index}:attribute#{ordinal}"),
                owner: node.attribute("owner")?.to_string(),
                title: node
                    .attribute("utf8title")
                    .or_else(|| node.attribute("title"))?
                    .to_string(),
                value: node
                    .attribute("utf8value")
                    .or_else(|| node.attribute("value"))?
                    .to_string(),
                value_type: node.attribute("type")?.to_string(),
                pdm_based: match node.attribute("pdmBased")? {
                    "true" => true,
                    "false" => false,
                    _ => return None,
                },
                version: node.attribute("version")?.parse().ok()?,
                source_entry: source_entry.to_string(),
                source_offset: entry_offset + node.range().start as u64,
            })
        })
        .collect()
}

/// Return the exact XML document carried by an NX XML stream.
///
/// NX permits one C-string terminator after the document. A terminator inside
/// the document or more than one trailing terminator rejects the whole stream.
fn xml_stream_text(payload: &[u8]) -> Option<&str> {
    let document = if let Some(document) = payload.strip_suffix(&[0]) {
        (!document.ends_with(&[0])).then_some(document)?
    } else {
        payload
    };
    (!document.contains(&0)).then_some(())?;
    std::str::from_utf8(document).ok()
}

/// Decode class definitions from every framed OM section.
pub fn class_definitions(container: &Container) -> Vec<ClassDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                class_layout_fields(definition.registry_suffix);
            definitions.insert(
                (entry_index, definition.offset),
                ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix,
                    schema_fingerprint,
                    layout_terminal,
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                class_layout_fields(definition.registry_suffix);
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix,
                    schema_fingerprint,
                    layout_terminal,
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

fn class_layout_fields(suffix: &[u8]) -> (Vec<u8>, Option<[u8; 8]>, Option<u8>) {
    if !(11..=14).contains(&suffix.len()) {
        return (Vec::new(), None, None);
    }
    let fingerprint_start = suffix.len() - 9;
    (
        suffix[..fingerprint_start].to_vec(),
        suffix[fingerprint_start..fingerprint_start + 8]
            .try_into()
            .ok(),
        suffix.last().copied(),
    )
}

/// Decode member definitions from every framed OM section.
pub fn field_definitions(container: &Container) -> Vec<FieldDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.fields.into_iter().enumerate() {
            definitions.insert(
                (entry_index, definition.offset),
                FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.fields.into_iter().enumerate() {
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

/// Catalog every externally bounded NX OM entity record.
pub fn object_records(container: &Container) -> Vec<ObjectRecord> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_offset = entry_offset + section.base_offset() as u64;
            let mut dependencies = BTreeMap::<usize, Vec<usize>>::new();
            let mut dependents = BTreeMap::<usize, Vec<usize>>::new();
            for (source, _, _, reference) in section.references() {
                if reference.kind != crate::om::ReferenceKind::RecordOrdinal16 {
                    continue;
                }
                let target = reference.value as usize;
                let outgoing = dependencies.entry(source).or_default();
                if !outgoing.contains(&target) {
                    outgoing.push(target);
                }
                let incoming = dependents.entry(target).or_default();
                if !incoming.contains(&source) {
                    incoming.push(source);
                }
            }
            section
                .records
                .into_iter()
                .enumerate()
                .map(move |(record_ordinal, record)| {
                    let record_id = |ordinal| {
                        format!("nx:om-record-directory-{section_ordinal}:entry#{ordinal}")
                    };
                    ObjectRecord {
                        id: record_id(record_ordinal),
                        object_id: record.object_id,
                        object_id_source_offset: record
                            .object_id_offset
                            .map(|offset| entry_offset + offset as u64),
                        section_ordinal: section_ordinal as u32,
                        record_ordinal: record_ordinal as u32,
                        section_offset,
                        byte_len: record.bytes.len() as u64,
                        sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                        dependencies: dependencies
                            .get(&record_ordinal)
                            .into_iter()
                            .flatten()
                            .map(|ordinal| record_id(*ordinal))
                            .collect(),
                        dependents: dependents
                            .get(&record_ordinal)
                            .into_iter()
                            .flatten()
                            .map(|ordinal| record_id(*ordinal))
                            .collect(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + record.offset as u64,
                    }
                })
                .collect()
        })
        .collect()
}

/// Retain the complete counted `RMFastLoad` active-object membership table.
pub fn rmfastload_object_id_table(
    container: &Container,
) -> Option<(RmFastLoadObjectIdTable, Vec<RmFastLoadObjectId>)> {
    let (entry, table) = container.rmfastload_object_id_table()?;
    let entry_offset = entry.file_span?.0;
    let table_id = "nx:rmfastload:object-id-table#0".to_string();
    let object_ids = table
        .object_ids
        .into_iter()
        .enumerate()
        .map(|(ordinal, object_id)| RmFastLoadObjectId {
            id: format!("nx:rmfastload:object-id#{ordinal:010}"),
            table: table_id.clone(),
            ordinal: ordinal as u32,
            value: object_id.value,
            raw: object_id.raw,
            source_offset: entry_offset + object_id.offset as u64,
        })
        .collect::<Vec<_>>();
    let native_table = RmFastLoadObjectIdTable {
        id: table_id,
        members: object_ids
            .iter()
            .map(|object_id| object_id.id.clone())
            .collect(),
        raw_count: table.raw_count,
        source_entry: entry.name.clone(),
        registry_source_offset: entry_offset + table.registry_offset as u64,
        source_offset: entry_offset + table.count_offset as u64,
    };
    Some((native_table, object_ids))
}

/// Catalog every externally bounded block in offset-only NX OM storage.
pub fn data_blocks(container: &Container) -> Vec<DataBlock> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_offset = entry_offset + section.base_offset() as u64;
            let mut source_blocks = Vec::with_capacity(section.records.len() + 1);
            if let Some(control) = section.control {
                source_blocks.push((DataBlockRole::Control, control));
            }
            source_blocks.extend(
                section
                    .records
                    .into_iter()
                    .map(|block| (DataBlockRole::Column, block)),
            );
            source_blocks
                .into_iter()
                .enumerate()
                .map(move |(block_ordinal, (role, block))| DataBlock {
                    id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    block_ordinal: block_ordinal as u32,
                    role,
                    section_offset,
                    byte_len: block.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(block.bytes),
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + block.offset as u64,
                })
                .collect()
        })
        .collect()
}

/// Decode complete zero-prefixed control arrays from offset-only OM stores.
pub fn data_block_control_values(container: &Container) -> Vec<DataBlockControlValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let Some(values) = crate::om::offset_store_control_values(control.bytes) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlValue {
                    id: format!(
                        "nx:om-data-block-control-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    value,
                    source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                })
                .collect()
        })
        .collect()
}

/// Resolve each atomic leading control lane through its store-local class registry.
pub fn data_block_control_class_references(
    container: &Container,
) -> Vec<DataBlockControlClassReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let registry = section.types;
            let Some(ordinals) = crate::om::offset_store_control_class_ordinals(
                control.bytes,
                registry.len(),
            ) else {
                return Vec::new();
            };
            let entry_index = container
                .entries
                .iter()
                .position(|candidate| std::ptr::eq(candidate, entry))
                .expect("indexed entry belongs to container");
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            ordinals
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, class_ordinal)| {
                    let definition = registry.get(usize::try_from(class_ordinal).ok()?)?;
                    Some(DataBlockControlClassReference {
                        id: format!(
                            "nx:om-data-block-control-class-references-{section_ordinal}:class#{ordinal}"
                        ),
                        data_block: data_block.clone(),
                        ordinal: ordinal as u32,
                        class_ordinal,
                        class_definition: format!(
                            "nx:om-entry-{entry_index}:class#{}",
                            definition.offset
                        ),
                        class_name: definition.name.to_string(),
                        source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode aligned index arrays ending at a unique control-block product record.
pub fn data_block_control_index_values(container: &Container) -> Vec<DataBlockControlIndexValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let Some((prefix_byte_len, values)) =
                crate::om::offset_store_index_values(control.bytes)
            else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            let block_count = section.records.len() + 1;
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlIndexValue {
                    id: format!(
                        "nx:om-data-block-control-index-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    prefix_byte_len: prefix_byte_len as u8,
                    value,
                    target_data_block: control_index_data_block(
                        section_ordinal,
                        block_count,
                        value,
                    ),
                    source_offset: entry_offset
                        + control.offset as u64
                        + prefix_byte_len as u64
                        + ordinal as u64 * 4,
                })
                .collect()
        })
        .collect()
}

pub(crate) fn control_index_data_block(
    section_ordinal: usize,
    block_count: usize,
    value: u32,
) -> Option<String> {
    let ordinal = usize::try_from(value)
        .ok()
        .filter(|ordinal| *ordinal < block_count)?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{ordinal}"
    ))
}

fn column_storage_block_at(
    section_ordinal: usize,
    records: &[crate::om::EntityRecord<'_>],
    offset: usize,
) -> Option<(String, u32)> {
    records.iter().enumerate().find_map(|(ordinal, record)| {
        let block_offset = offset.checked_sub(record.offset)?;
        if block_offset >= record.bytes.len() {
            return None;
        }
        let block_offset = u32::try_from(block_offset).ok()?;
        Some((
            format!("nx:om-data-blocks-{section_ordinal}:block#{}", ordinal + 1),
            block_offset,
        ))
    })
}

/// Decode persistent-handle and tagged-28 occurrences in bounded control blocks.
pub fn data_block_control_references(container: &Container) -> Vec<DataBlockControlReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            crate::om::references(control.bytes, control.offset)
                .into_iter()
                .filter(|reference| reference.kind != crate::om::ReferenceKind::RecordOrdinal16)
                .enumerate()
                .map(|(ordinal, reference)| DataBlockControlReference {
                    id: format!(
                        "nx:om-data-block-control-references-{section_ordinal}:reference#{}",
                        reference.offset
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    kind: match reference.kind {
                        crate::om::ReferenceKind::PersistentHandle => {
                            ObjectReferenceKind::PersistentHandle
                        }
                        crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                        crate::om::ReferenceKind::RecordOrdinal16 => unreachable!("filtered"),
                    },
                    value: reference.value,
                    source_offset: entry_offset + reference.offset as u64,
                })
                .collect()
        })
        .collect()
}

/// Join maximal two-token adjacent persistent-handle runs atomically.
pub fn data_block_control_handle_pairs(
    references: &[DataBlockControlReference],
) -> Vec<DataBlockControlHandlePair> {
    let mut by_block = BTreeMap::<&str, Vec<&DataBlockControlReference>>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        by_block
            .entry(reference.data_block.as_str())
            .or_default()
            .push(reference);
    }
    let mut pairs = Vec::new();
    for (data_block, mut block_references) in by_block {
        block_references.sort_by_key(|reference| reference.source_offset);
        let mut at = 0;
        while at < block_references.len() {
            let start = at;
            while block_references
                .get(at + 1)
                .is_some_and(|next| next.source_offset == block_references[at].source_offset + 5)
            {
                at += 1;
            }
            let run = &block_references[start..=at];
            if let [first, second] = run {
                pairs.push(DataBlockControlHandlePair {
                    id: format!(
                        "nx:om-data-block-control:handle-pair#{}",
                        first.source_offset
                    ),
                    data_block: data_block.to_string(),
                    first_reference: first.id.clone(),
                    second_reference: second.id.clone(),
                    first_handle: first.value,
                    second_handle: second.value,
                    source_offset: first.source_offset,
                });
            }
            at += 1;
        }
    }
    pairs
}

/// Decode framed object references from offset-only OM data blocks.
pub fn data_block_references(container: &Container) -> Vec<DataBlockReference> {
    let mut records = BTreeMap::<(String, u32), Vec<String>>::new();
    for record in object_records(container) {
        let Some(object_id) = record.object_id else {
            continue;
        };
        records
            .entry((record.source_entry, object_id))
            .or_default()
            .push(record.id);
    }
    let mut declarations = BTreeMap::<(String, u32), Vec<String>>::new();
    for declaration in expression_declarations(container) {
        declarations
            .entry((declaration.source_entry, declaration.object_id))
            .or_default()
            .push(declaration.id);
    }
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let mut source_blocks = Vec::with_capacity(section.records.len() + 1);
            if let Some(control) = section.control {
                source_blocks.push(control);
            }
            source_blocks.extend(section.records);
            source_blocks
                .into_iter()
                .enumerate()
                .flat_map(|(block_ordinal, block)| {
                    crate::om::data_block_object_references(block.bytes)
                        .into_iter()
                        .enumerate()
                        .map(|(ordinal, reference)| {
                            let key = (entry.name.clone(), reference.object_index);
                            let unique = |candidates: Option<&Vec<String>>| {
                                let [target] = candidates?.as_slice() else {
                                    return None;
                                };
                                Some(target.clone())
                            };
                            DataBlockReference {
                                id: format!(
                                    "nx:om-data-block-references-{section_ordinal}-{block_ordinal}:reference#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                object_id: reference.object_index,
                                raw_object_id: reference.raw_object_index,
                                target_record: unique(records.get(&key)),
                                target_expression_declaration: unique(declarations.get(&key)),
                                source_offset: entry_offset
                                    + block.offset as u64
                                    + reference.offset as u64,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range counted block-index lanes from offset-only stores.
pub fn data_block_counted_index_lanes(container: &Container) -> Vec<DataBlockCountedIndexLane> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let block_count = section.records.len() + 1;
            section
                .records
                .into_iter()
                .enumerate()
                .flat_map(|(record_ordinal, block)| {
                    let block_ordinal = record_ordinal + 1;
                    crate::om::offset_store_counted_index_lanes(block.bytes)
                        .into_iter()
                        .filter_map(|lane| {
                            let anchor_data_block = control_index_data_block(
                                section_ordinal,
                                block_count,
                                lane.anchor,
                            )?;
                            let member_data_blocks = lane
                                .members
                                .iter()
                                .map(|(value, _)| {
                                    control_index_data_block(
                                        section_ordinal,
                                        block_count,
                                        *value,
                                    )
                                })
                                .collect::<Option<Vec<_>>>()?;
                            let source_base = entry_offset + block.offset as u64;
                            Some((lane, anchor_data_block, member_data_blocks, source_base))
                        })
                        .enumerate()
                        .map(
                            |(
                                ordinal,
                                (lane, anchor_data_block, member_data_blocks, source_base),
                            )| DataBlockCountedIndexLane {
                                id: format!(
                                    "nx:om-data-block-counted-index-lanes-{section_ordinal}-{block_ordinal}:lane#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                declared_count: lane.declared_count,
                                anchor_index: lane.anchor,
                                raw_anchor_index: lane.raw_anchor,
                                anchor_data_block,
                                member_indices: lane
                                    .members
                                    .iter()
                                    .map(|(value, _)| *value)
                                    .collect(),
                                raw_member_indices: lane.raw_members,
                                member_data_blocks,
                                source_offset: source_base + lane.offset as u64,
                                anchor_source_offset: source_base + lane.anchor_offset as u64,
                                member_source_offsets: lane
                                    .members
                                    .iter()
                                    .map(|(_, offset)| source_base + *offset as u64)
                                    .collect(),
                            },
                        )
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range `ABR` reference lanes from offset-store column storage.
pub fn data_block_abr_reference_lanes(container: &Container) -> Vec<DataBlockAbrReferenceLane> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let source_base = entry_offset + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_abr_reference_lanes(storage)
                .into_iter()
                .filter_map(|lane| {
                    let slot_data_blocks = lane
                        .slots
                        .iter()
                        .map(|(value, _)| {
                            value.map_or(Some(None), |value| {
                                control_index_data_block(section_ordinal, block_count, value)
                                    .map(Some)
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some((lane, slot_data_blocks))
                })
                .enumerate()
                .map(
                    |(ordinal, (lane, slot_data_blocks))| DataBlockAbrReferenceLane {
                        id: format!(
                            "nx:om-data-block-abr-reference-lanes-{section_ordinal}:lane#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        slot_indices: lane.slots.iter().map(|(value, _)| *value).collect(),
                        raw_slot_indices: lane.raw_slots,
                        slot_data_blocks,
                        slot_source_offsets: lane
                            .slots
                            .iter()
                            .map(|(_, offset)| source_base + *offset as u64)
                            .collect(),
                        source_entry: entry.name.clone(),
                        source_offset: source_base + lane.offset as u64,
                    },
                )
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete index rows from offset-store column storage.
pub fn data_block_index_rows(container: &Container) -> Vec<DataBlockIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let data_blocks = row
                        .indices
                        .iter()
                        .map(|(index, _)| {
                            control_index_data_block(section_ordinal, block_count, *index)
                        })
                        .collect::<Option<Vec<_>>>()
                        .and_then(|blocks| blocks.try_into().ok())?;
                    let opening = column_storage_block_at(
                        section_ordinal,
                        &section.records,
                        storage_offset + row.offset,
                    )?;
                    Some((row, data_blocks, opening))
                })
                .enumerate()
                .map(|(ordinal, (row, data_blocks, opening))| DataBlockIndexRow {
                    id: format!("nx:om-data-block-index-rows-{section_ordinal}:row#{ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    ordinal: ordinal as u32,
                    first_index: row.first_index,
                    raw_first_index: row.raw_first_index,
                    flag: row.flag,
                    indices: row.indices.map(|(index, _)| index),
                    raw_indices: row.raw_indices,
                    data_blocks,
                    source_entry: entry.name.clone(),
                    opening_data_block: opening.0,
                    opening_block_offset: opening.1,
                    source_offset: source_base + row.offset as u64,
                    first_index_source_offset: source_base + row.first_index_offset as u64,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range linked index rows from column storage.
pub fn data_block_linked_index_rows(container: &Container) -> Vec<DataBlockLinkedIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_linked_index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let values = std::iter::once(row.target_index.0)
                        .chain(row.indices.iter().map(|(index, _)| *index));
                    let data_blocks = values
                        .map(|index| control_index_data_block(section_ordinal, block_count, index))
                        .collect::<Option<Vec<_>>>()
                        .and_then(|blocks| blocks.try_into().ok())?;
                    let opening = column_storage_block_at(
                        section_ordinal,
                        &section.records,
                        storage_offset + row.offset,
                    )?;
                    Some((row, data_blocks, opening))
                })
                .enumerate()
                .map(
                    |(ordinal, (row, data_blocks, opening))| DataBlockLinkedIndexRow {
                        id: format!(
                            "nx:om-data-block-linked-index-rows-{section_ordinal}:row#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        first_index: row.first_index.0,
                        raw_first_index: row.raw_first_index,
                        discriminator: row.discriminator,
                        target_index: row.target_index.0,
                        raw_target_index: row.raw_target_index,
                        indices: row.indices.map(|(index, _)| index),
                        raw_indices: row.raw_indices,
                        data_blocks,
                        flag: row.flag,
                        mode: row.mode,
                        source_entry: entry.name.clone(),
                        opening_data_block: opening.0,
                        opening_block_offset: opening.1,
                        source_offset: source_base + row.offset as u64,
                        first_index_source_offset: source_base + row.first_index.1 as u64,
                        target_index_source_offset: source_base + row.target_index.1 as u64,
                        index_source_offsets: row
                            .indices
                            .map(|(_, offset)| source_base + offset as u64),
                    },
                )
                .collect()
        })
        .collect()
}

/// Decode complete in-range target-index rows from column storage.
pub fn data_block_target_index_rows(container: &Container) -> Vec<DataBlockTargetIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_target_index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let values = std::iter::once(row.target_index.0)
                        .chain(row.indices.iter().map(|(index, _)| *index));
                    let data_blocks = values
                        .map(|index| control_index_data_block(section_ordinal, block_count, index))
                        .collect::<Option<Vec<_>>>()
                        .and_then(|blocks| blocks.try_into().ok())?;
                    let opening = column_storage_block_at(
                        section_ordinal,
                        &section.records,
                        storage_offset + row.offset,
                    )?;
                    Some((row, data_blocks, opening))
                })
                .enumerate()
                .map(
                    |(ordinal, (row, data_blocks, opening))| DataBlockTargetIndexRow {
                        id: format!(
                            "nx:om-data-block-target-index-rows-{section_ordinal}:row#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        target_index: row.target_index.0,
                        raw_target_index: row.raw_target_index,
                        indices: row.indices.map(|(index, _)| index),
                        raw_indices: row.raw_indices,
                        data_blocks,
                        mode: row.mode,
                        source_entry: entry.name.clone(),
                        opening_data_block: opening.0,
                        opening_block_offset: opening.1,
                        source_offset: source_base + row.offset as u64,
                        target_index_source_offset: source_base + row.target_index.1 as u64,
                        index_source_offsets: row
                            .indices
                            .map(|(_, offset)| source_base + offset as u64),
                    },
                )
                .collect()
        })
        .collect()
}

/// Resolve complete composite column-index tables atomically by section.
pub fn data_block_column_index_tables(
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Vec<DataBlockColumnIndexTable> {
    let mut linked_by_section = BTreeMap::<u32, Vec<&DataBlockLinkedIndexRow>>::new();
    for row in linked_rows {
        linked_by_section
            .entry(row.section_ordinal)
            .or_default()
            .push(row);
    }
    let mut targets_by_section = BTreeMap::<u32, Vec<&DataBlockTargetIndexRow>>::new();
    for row in target_rows {
        targets_by_section
            .entry(row.section_ordinal)
            .or_default()
            .push(row);
    }
    linked_by_section
        .into_iter()
        .filter_map(|(section_ordinal, linked)| {
            let targets = targets_by_section.remove(&section_ordinal)?;
            let (opening, suffix) = linked.split_first()?;
            let (last_target, target_prefix) = targets.split_last()?;
            if opening.mode != 7
                || suffix.is_empty()
                || suffix.iter().any(|row| row.mode != 4)
                || last_target.mode != 4
                || target_prefix.iter().any(|row| row.mode != 7)
            {
                return None;
            }
            let ordered = std::iter::once((opening.target_index, opening.source_offset))
                .chain(
                    targets
                        .iter()
                        .map(|row| (row.target_index, row.source_offset)),
                )
                .chain(
                    suffix
                        .iter()
                        .map(|row| (row.target_index, row.source_offset)),
                )
                .collect::<Vec<_>>();
            if ordered
                .windows(2)
                .any(|pair| pair[0].0.checked_sub(1) != Some(pair[1].0) || pair[0].1 >= pair[1].1)
                || linked
                    .iter()
                    .any(|row| row.source_entry != opening.source_entry)
                || targets
                    .iter()
                    .any(|row| row.source_entry != opening.source_entry)
            {
                return None;
            }
            Some(DataBlockColumnIndexTable {
                id: format!("nx:om-data-block-column-index-tables:table#{section_ordinal}"),
                section_ordinal,
                opening_linked_row: opening.id.clone(),
                target_rows: targets.iter().map(|row| row.id.clone()).collect(),
                linked_rows: suffix.iter().map(|row| row.id.clone()).collect(),
                first_target_index: opening.target_index,
                last_target_index: ordered.last().expect("nonempty column table").0,
                source_entry: opening.source_entry.clone(),
                source_offset: opening.source_offset,
            })
        })
        .collect()
}

/// Decode one product/version header from each indexed NX OM store.
pub fn store_headers(container: &Container) -> Vec<StoreHeader> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .filter_map(|(section_ordinal, (entry, section))| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .control
                .iter()
                .chain(section.records.iter())
                .find_map(|record| {
                    crate::om::store_version(record.bytes, record.offset).map(|version| {
                        StoreHeader {
                            id: format!("nx:om-store-headers:store#{section_ordinal}"),
                            section_ordinal: section_ordinal as u32,
                            object_id: record.object_id,
                            version: version.value.to_string(),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + version.offset as u64,
                        }
                    })
                })
        })
        .collect()
}

/// Decode self-framed printable values from bounded NX OM records.
pub fn string_values(container: &Container) -> Vec<StringValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .string_values()
                .into_iter()
                .map(move |(record_ordinal, value_ordinal, object_id, value)| {
                    let record =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    StringValue {
                        id: format!(
                            "nx:om-string-values-{section_ordinal}-{record_ordinal}:value#{}",
                            value.offset
                        ),
                        record,
                        object_id,
                        ordinal: value_ordinal as u32,
                        value: value.value.to_string(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + value.offset as u64,
                    }
                })
                .collect()
        })
        .collect()
}

/// Decode ordered tagged references from bounded NX OM records.
pub fn object_references(container: &Container) -> Vec<ObjectReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .references()
                .into_iter()
                .map(
                    move |(record_ordinal, reference_ordinal, object_id, reference)| {
                        let record = format!(
                            "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                        );
                        ObjectReference {
                            id: format!(
                                "nx:om-references-{section_ordinal}-{record_ordinal}:reference#{}",
                                reference.offset
                            ),
                            record,
                            object_id,
                            ordinal: reference_ordinal as u32,
                            kind: match reference.kind {
                                crate::om::ReferenceKind::PersistentHandle => {
                                    ObjectReferenceKind::PersistentHandle
                                }
                                crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                                crate::om::ReferenceKind::RecordOrdinal16 => {
                                    ObjectReferenceKind::RecordOrdinal16
                                }
                            },
                            value: reference.value,
                            target_record: (reference.kind
                                == crate::om::ReferenceKind::RecordOrdinal16)
                                .then(|| {
                                    format!(
                                        "nx:om-record-directory-{section_ordinal}:entry#{}",
                                        reference.value
                                    )
                                }),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + reference.offset as u64,
                        }
                    },
                )
                .collect()
        })
        .collect()
}

/// Group persistent-handle occurrences into cross-record identities.
pub fn persistent_handles(
    references: &[ObjectReference],
    control_references: &[DataBlockControlReference],
    external: &[ExternalReferenceRecord],
    external_tail_pairs: &[ExternalReferenceTailReferencePair],
) -> Vec<PersistentHandle> {
    #[derive(Default)]
    struct Group {
        records: Vec<String>,
        occurrence_count: u32,
        external_records: Vec<String>,
        data_blocks: Vec<String>,
        external_occurrence_count: u32,
    }

    let mut groups = BTreeMap::<u32, Group>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let group = groups.entry(reference.value).or_default();
        group.occurrence_count += 1;
        if group.records.last() != Some(&reference.record)
            && !group.records.contains(&reference.record)
        {
            group.records.push(reference.record.clone());
        }
    }
    for reference in control_references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let group = groups.entry(reference.value).or_default();
        group.occurrence_count += 1;
        if !group.data_blocks.contains(&reference.data_block) {
            group.data_blocks.push(reference.data_block.clone());
        }
    }
    for record in external {
        for handle in &record.handles {
            let group = groups.entry(*handle).or_default();
            group.external_occurrence_count += 1;
            if !group.external_records.contains(&record.id) {
                group.external_records.push(record.id.clone());
            }
        }
        if record.closing_duplicate {
            let Some(handle) = record.handles.last() else {
                continue;
            };
            groups.entry(*handle).or_default().external_occurrence_count += 1;
        }
    }
    for pair in external_tail_pairs {
        let group = groups.entry(pair.persistent_handle).or_default();
        group.external_occurrence_count += 1;
        if !group.external_records.contains(&pair.handle_set_record) {
            group.external_records.push(pair.handle_set_record.clone());
        }
    }
    groups
        .into_iter()
        .map(|(value, group)| PersistentHandle {
            id: format!("nx:om-persistent-handles:handle#{value:08x}"),
            value,
            records: group.records,
            occurrence_count: group.occurrence_count,
            data_blocks: group.data_blocks,
            external_records: group.external_records,
            external_occurrence_count: group.external_occurrence_count,
        })
        .collect()
}

/// Decode named parameter declarations from expression-class OM records.
pub fn expression_declarations(container: &Container) -> Vec<ExpressionDeclaration> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if !section
                .types
                .iter()
                .any(|definition| definition.name == "UGS::EXP_expression")
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .records
                .into_iter()
                .enumerate()
                .filter_map(|(record_ordinal, record)| {
                    let object_id = record.object_id?;
                    let declaration = crate::om::expression_declaration_name(record.bytes)?;
                    let record_id =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    Some(ExpressionDeclaration {
                        id: format!(
                            "nx:om-expression-declarations-{section_ordinal}:declaration#{record_ordinal}"
                        ),
                        object_id,
                        record: record_id,
                        name: declaration.value.to_string(),
                        parameter_index: declaration.parameter_index,
                        qualifier: declaration.qualifier.map(str::to_string),
                        literal: declaration.literal.map(str::to_string),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset
                            + record.offset as u64
                            + declaration.offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode explicit numeric expressions from all indexed OM sections.
pub fn expressions(container: &Container) -> Vec<Expression> {
    let declarations = expression_declarations(container);
    let mut declarations_by_name = BTreeMap::<(&str, &str), Vec<&ExpressionDeclaration>>::new();
    for declaration in &declarations {
        declarations_by_name
            .entry((declaration.source_entry.as_str(), declaration.name.as_str()))
            .or_default()
            .push(declaration);
    }
    let mut indexed = BTreeMap::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        for (record_ordinal, expression) in section.numeric_expression_records() {
            let Some(object_id) = expression.object_id else {
                continue;
            };
            indexed.insert(
                (entry.name.clone(), expression.offset),
                (
                    Some(object_id),
                    format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                ),
            );
        }
    }
    let mut expressions = Vec::new();
    for (entry_index, entry) in container.entries.iter().enumerate() {
        let Some((entry_offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(entry_offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = container.data.get(offset..offset.saturating_add(size)) else {
            continue;
        };
        for expression in crate::om::numeric_expressions(payload) {
            let Some(table_offset) = payload[..expression.offset]
                .windows(b"hostglobalvariables".len())
                .rposition(|window| window == b"hostglobalvariables")
            else {
                continue;
            };
            let indexed_record = indexed
                .get(&(entry.name.clone(), expression.offset))
                .cloned();
            let declaration = declarations_by_name
                .get(&(entry.name.as_str(), expression.name))
                .and_then(|candidates| {
                    let same_record_arena = |first: &str, second: &str| {
                        first.split_once(":entry#").map(|pair| pair.0)
                            == second.split_once(":entry#").map(|pair| pair.0)
                    };
                    let candidates = candidates
                        .iter()
                        .copied()
                        .filter(|declaration| {
                            indexed_record.as_ref().is_none_or(|(_, record)| {
                                same_record_arena(&declaration.record, record)
                            })
                        })
                        .collect::<Vec<_>>();
                    let [declaration] = candidates.as_slice() else {
                        return None;
                    };
                    Some(declaration.id.clone())
                });
            expressions.push(Expression {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                object_id: indexed_record
                    .as_ref()
                    .and_then(|(object_id, _)| *object_id),
                record: indexed_record.map(|(_, record)| record),
                declaration,
                name: expression.name.to_string(),
                parameter_index: expression.parameter_index,
                qualifier: expression.qualifier.map(str::to_string),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                },
                expression: expression.expression.to_string(),
                value: expression.value,
                source_entry: entry.name.clone(),
                source_table: format!("nx:om-entry-{entry_index}:expression-table#{table_offset}"),
                source_offset: entry_offset + expression.offset as u64,
            });
        }
    }
    evaluate_expression_graphs(&mut expressions);
    expressions
}

fn expression_scope(expression: &Expression) -> &str {
    if expression.source_table.is_empty() {
        &expression.source_entry
    } else {
        &expression.source_table
    }
}

pub(crate) fn evaluate_expression_graphs(expressions: &mut [Expression]) {
    let mut name_counts = BTreeMap::<(String, String), usize>::new();
    for expression in expressions.iter() {
        *name_counts
            .entry((
                expression_scope(expression).to_string(),
                expression.name.clone(),
            ))
            .or_default() += 1;
    }
    let mut values = BTreeMap::<(String, String), (ExpressionUnit, f64)>::new();
    for expression in expressions.iter_mut() {
        let key = (
            expression_scope(expression).to_string(),
            expression.name.clone(),
        );
        if name_counts.get(&key) != Some(&1) {
            expression.value = None;
            continue;
        }
        if let Some(value) = expression.value {
            values.insert(key, (expression.unit, value));
        }
    }

    loop {
        let mut changed = false;
        for expression in expressions
            .iter_mut()
            .filter(|expression| expression.value.is_none())
        {
            let expression_key = (
                expression_scope(expression).to_string(),
                expression.name.clone(),
            );
            if name_counts.get(&expression_key) != Some(&1) {
                continue;
            }
            let mut substituted = String::with_capacity(expression.expression.len());
            let bytes = expression.expression.as_bytes();
            let mut at = 0usize;
            let mut complete = true;
            while at < bytes.len() {
                if let Some(end) = expression_parameter_reference_end(bytes, at) {
                    let start = at;
                    at = end;
                    let name = &expression.expression[start..at];
                    let key = (expression_scope(expression).to_string(), name.to_string());
                    let Some((unit, value)) = values.get(&key).copied() else {
                        complete = false;
                        break;
                    };
                    if name_counts.get(&key) != Some(&1) || expression.unit != unit {
                        complete = false;
                        break;
                    }
                    substituted.push('(');
                    substituted.push_str(&value.to_string());
                    substituted.push(')');
                    continue;
                }
                substituted.push(char::from(bytes[at]));
                at += 1;
            }
            if complete {
                if let Some(value) = crate::om::evaluate_constant_expression(&substituted) {
                    expression.value = Some(value);
                    values.insert(expression_key.clone(), (expression.unit, value));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn nx_expression_parameter_references_preserve_formula_order() {
        assert_eq!(
            super::expression_parameter_names(
                "max(p12, p3) + p12 + exp2 + p7_radius + p7_radius + p4bad + p5_"
            ),
            vec!["p12", "p3", "p12", "p7_radius", "p7_radius"]
        );
    }

    #[test]
    fn nx_expression_graph_rejects_noncanonical_parameter_tokens() {
        let expression = |name: &str, formula: &str, value| super::Expression {
            id: format!("nx:test:expression#{name}"),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: "table".into(),
            source_offset: 0,
        };
        let mut expressions = vec![
            expression("p4", "3", Some(3.0)),
            expression("p5", "p4bad + 2", None),
            expression("p6", "p4_ + 2", None),
        ];

        super::evaluate_expression_graphs(&mut expressions);

        assert_eq!(expressions[1].value, None);
        assert_eq!(expressions[2].value, None);
    }

    #[test]
    fn nx_expression_graph_evaluates_exact_qualified_dependencies() {
        let expression = |name: &str, formula: &str, value| super::Expression {
            id: format!("nx:test:expression#{name}"),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: "table".into(),
            source_offset: 0,
        };
        let mut expressions = vec![
            expression("p7", "3", Some(3.0)),
            expression("p7_radius", "5", Some(5.0)),
            expression("p8", "p7_radius * 2", None),
            expression("p9", "p8 + p7", None),
        ];

        super::evaluate_expression_graphs(&mut expressions);

        assert_eq!(expressions[2].value, Some(10.0));
        assert_eq!(expressions[3].value, Some(13.0));
    }

    #[test]
    fn nx_expression_graph_substitutes_dependencies_as_atomic_operands() {
        let expression = |name: &str, formula: &str, value| super::Expression {
            id: format!("nx:test:expression#{name}"),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: "table".into(),
            source_offset: 0,
        };
        let mut expressions = vec![
            expression("p1", "-2", Some(-2.0)),
            expression("p2", "p1^2", None),
            expression("p3", "-p1^2", None),
        ];

        super::evaluate_expression_graphs(&mut expressions);

        assert_eq!(expressions[1].value, Some(4.0));
        assert_eq!(expressions[2].value, Some(-4.0));
    }

    #[test]
    fn nx_expression_graph_scopes_names_to_their_expression_table() {
        let expression =
            |id: &str, table: &str, name: &str, formula: &str, value| super::Expression {
                id: id.into(),
                object_id: None,
                record: None,
                declaration: None,
                name: name.into(),
                parameter_index: None,
                qualifier: None,
                unit: super::ExpressionUnit::Millimeter,
                expression: formula.into(),
                value,
                source_entry: "part".into(),
                source_table: table.into(),
                source_offset: 0,
            };
        let mut expressions = vec![
            expression("a-p2", "table-a", "p2", "5", Some(5.0)),
            expression("a-p3", "table-a", "p3", "p2 * 2", None),
            expression("b-p2", "table-b", "p2", "7", Some(7.0)),
            expression("b-p3", "table-b", "p3", "p2 * 2", None),
        ];

        super::evaluate_expression_graphs(&mut expressions);

        assert_eq!(expressions[1].value, Some(10.0));
        assert_eq!(expressions[3].value, Some(14.0));
    }

    #[test]
    fn nx_expression_graph_rejects_every_duplicate_name_in_one_table() {
        let expression =
            |id: &str, table: &str, name: &str, formula: &str, value| super::Expression {
                id: id.into(),
                object_id: None,
                record: None,
                declaration: None,
                name: name.into(),
                parameter_index: None,
                qualifier: None,
                unit: super::ExpressionUnit::Millimeter,
                expression: formula.into(),
                value,
                source_entry: "part".into(),
                source_table: table.into(),
                source_offset: 0,
            };
        let mut expressions = vec![
            expression("a-p1-first", "table-a", "p1", "3", Some(3.0)),
            expression("a-p1-second", "table-a", "p1", "5", Some(5.0)),
            expression("a-p2", "table-a", "p2", "p1 * 2", None),
            expression("b-p1", "table-b", "p1", "7", Some(7.0)),
            expression("b-p2", "table-b", "p2", "p1 * 2", None),
        ];

        super::evaluate_expression_graphs(&mut expressions);

        assert_eq!(expressions[0].value, None);
        assert_eq!(expressions[1].value, None);
        assert_eq!(expressions[2].value, None);
        assert_eq!(expressions[3].value, Some(7.0));
        assert_eq!(expressions[4].value, Some(14.0));
    }

    #[test]
    fn nx_formula_dependencies_resolve_to_section_parameters() {
        let expression = |key: u32,
                          name: &str,
                          index: u32,
                          qualifier: Option<&str>,
                          text: &str,
                          value: Option<f64>| super::Expression {
            id: format!("nx:test:expression#{key}"),
            object_id: Some(key),
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: Some(index),
            qualifier: qualifier.map(str::to_string),
            unit: super::ExpressionUnit::Millimeter,
            expression: text.into(),
            value,
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: "table".into(),
            source_offset: u64::from(key),
        };
        let expressions = [
            expression(20, "p2", 2, None, "5", Some(5.0)),
            expression(21, "p2_radius", 2, Some("radius"), "7", Some(7.0)),
            expression(90, "p9", 9, None, "p2_radius * 2 + p2_radius", None),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert_eq!(ir.model.parameters[2].value, None);
        assert_eq!(
            ir.model.parameters[2].dependencies,
            vec![ir.model.parameters[1].id.clone()]
        );
    }

    #[test]
    fn nx_formula_dependencies_reject_ambiguous_parameter_names() {
        let expression = |key: u32, name: &str, text: &str| super::Expression {
            id: format!("nx:test:expression#{key}"),
            object_id: Some(key),
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: Some(key),
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: text.into(),
            value: None,
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: "table".into(),
            source_offset: u64::from(key),
        };
        let expressions = [
            expression(20, "p2", "5"),
            expression(21, "p2", "7"),
            expression(90, "p9", "p2 * 2"),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert!(ir.model.parameters[2].dependencies.is_empty());
    }

    #[test]
    fn nx_formula_dependencies_resolve_within_the_expression_table() {
        let expression =
            |id: &str, table: &str, name: &str, text: &str, source_offset: u64| super::Expression {
                id: format!("nx:test:expression#{id}"),
                object_id: None,
                record: None,
                declaration: None,
                name: name.into(),
                parameter_index: None,
                qualifier: None,
                unit: super::ExpressionUnit::Millimeter,
                expression: text.into(),
                value: None,
                source_entry: "/Root/UG_PART/UG_PART".into(),
                source_table: table.into(),
                source_offset,
            };
        let expressions = [
            expression("a-p3", "table-a", "p3", "p2 * 2", 40),
            expression("b-p3", "table-b", "p3", "p2 * 2", 10),
            expression("a-p2", "table-a", "p2", "5", 30),
            expression("b-p2", "table-b", "p2", "7", 20),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert_eq!(ir.model.features.len(), 2);
        assert_eq!(ir.model.features[0].id.0, "table-b:feature#equations");
        assert_eq!(ir.model.features[0].ordinal, 0);
        assert_eq!(ir.model.features[1].id.0, "table-a:feature#equations");
        assert_eq!(ir.model.features[1].ordinal, 1);
        assert_eq!(
            ir.model
                .parameters
                .iter()
                .map(|parameter| (parameter.name.as_str(), parameter.ordinal))
                .collect::<Vec<_>>(),
            [("p2", 0), ("p3", 1), ("p2", 0), ("p3", 1)]
        );
        assert_eq!(ir.model.parameters[1].owner, ir.model.parameters[0].owner);
        assert_eq!(
            ir.model.parameters[1].dependencies,
            [ir.model.parameters[0].id.clone()]
        );
        assert_eq!(ir.model.parameters[3].owner, ir.model.parameters[2].owner);
        assert_eq!(
            ir.model.parameters[3].dependencies,
            [ir.model.parameters[2].id.clone()]
        );
        assert_ne!(ir.model.parameters[1].owner, ir.model.parameters[3].owner);
        for parameter in &mut ir.model.parameters {
            parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(1.0),
            ));
        }
        assert!(crate::decode::incomplete_expression_parameters(&ir).is_empty());

        let mut duplicate_name = ir.clone();
        duplicate_name.model.parameters[1].name = duplicate_name.model.parameters[0].name.clone();
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&duplicate_name),
            duplicate_name.model.parameters[..2]
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect()
        );

        let mut unevaluated = ir.clone();
        unevaluated.model.parameters[1].value = None;
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&unevaluated),
            [unevaluated.model.parameters[1].id.clone()].into()
        );

        let mut operation_owned = unevaluated;
        operation_owned.model.features[0].definition =
            cadmpeg_ir::features::FeatureDefinition::Native {
                kind: "TEST_OPERATION".into(),
                properties: Default::default(),
                parameters: Default::default(),
            };
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&operation_owned),
            [operation_owned.model.parameters[1].id.clone()].into()
        );
    }

    #[test]
    fn nx_cyclic_formula_table_omits_invalid_neutral_dependency_edges() {
        let expression = |id: &str, name: &str, text: &str, source_offset| super::Expression {
            id: format!("nx:test:expression#{id}"),
            object_id: None,
            record: None,
            declaration: None,
            name: name.to_string(),
            parameter_index: None,
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: text.to_string(),
            value: None,
            source_entry: "part".to_string(),
            source_table: "table".to_string(),
            source_offset,
        };
        let expressions = [
            expression("p2", "p2", "p3 + 1", 10),
            expression("p3", "p3", "p2 + 1", 20),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert_eq!(ir.model.parameters[0].expression, "p3 + 1");
        assert_eq!(ir.model.parameters[1].expression, "p2 + 1");
        assert!(ir
            .model
            .parameters
            .iter()
            .all(|parameter| parameter.dependencies.is_empty()));
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&ir),
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect()
        );
        let mut losses = Vec::new();
        crate::decode::append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("2 NX expression parameter(s)"));
    }

    #[test]
    fn nx_cyclic_formula_table_retains_independent_acyclic_dependencies() {
        let expression = |id: &str, name: &str, text: &str, source_offset| super::Expression {
            id: format!("nx:test:expression#{id}"),
            object_id: None,
            record: None,
            declaration: None,
            name: name.to_string(),
            parameter_index: None,
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: text.to_string(),
            value: None,
            source_entry: "part".to_string(),
            source_table: "table".to_string(),
            source_offset,
        };
        let expressions = [
            expression("p2", "p2", "p3 + 1", 10),
            expression("p3", "p3", "p2 + 1", 20),
            expression("p5", "p5", "p4 * 2", 40),
            expression("p4", "p4", "7", 30),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert_eq!(
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["p4", "p5", "p2", "p3"]
        );
        assert_eq!(
            ir.model.parameters[1].dependencies,
            [ir.model.parameters[0].id.clone()]
        );
        assert!(ir.model.parameters[2].dependencies.is_empty());
        assert!(ir.model.parameters[3].dependencies.is_empty());
        for parameter in &mut ir.model.parameters {
            parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(1.0),
            ));
        }
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&ir),
            ir.model.parameters[2..]
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect()
        );
    }

    #[test]
    fn nx_parameter_uses_group_binding_witnesses_and_project_consumers() {
        use crate::native::features::{feature_parameter_uses, FeatureParameterBinding};

        let binding = |id: &str, operation: &str, slot: u8, offset: u64| FeatureParameterBinding {
            id: id.to_string(),
            operation_label: operation.to_string(),
            input_slot: slot,
            input_block: format!("block-{slot}"),
            reference_ordinal: 0,
            expression_declaration: "declaration".to_string(),
            expression: Some("nx:test:expression#20".to_string()),
            object_id: 20,
            source_offset: offset,
        };
        let uses = feature_parameter_uses(&[
            binding("late", "nx:feature-history:operation-label#1-2", 1, 30),
            binding("early", "nx:feature-history:operation-label#1-2", 0, 20),
            binding("other", "nx:feature-history:operation-label#1-3", 0, 40),
        ]);
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].bindings, ["early", "late"]);
        assert_eq!(uses[0].source_offsets, [20, 30]);

        let expression = super::Expression {
            id: "nx:test:expression#20".to_string(),
            object_id: Some(20),
            record: None,
            declaration: None,
            name: "p20".to_string(),
            parameter_index: Some(20),
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: "5".to_string(),
            value: Some(5.0),
            source_entry: "part".to_string(),
            source_table: "table".to_string(),
            source_offset: 20,
        };
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &[expression],
            &[],
            &uses,
            &mut annotations,
        );
        assert_eq!(
            ir.model.parameters[0].properties["consumer.0"],
            "nx:feature-history:feature#1-2"
        );
        assert_eq!(
            ir.model.parameters[0].properties["consumer.1"],
            "nx:feature-history:feature#1-3"
        );
    }

    #[test]
    fn nx_parameter_consumers_follow_physical_use_order() {
        let expression = super::Expression {
            id: "nx:test:expression#20".to_string(),
            object_id: Some(20),
            record: None,
            declaration: None,
            name: "p20".to_string(),
            parameter_index: Some(20),
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: "5".to_string(),
            value: Some(5.0),
            source_entry: "part".to_string(),
            source_table: "table".to_string(),
            source_offset: 10,
        };
        let parameter_use = |id: &str, operation: &str, source_offset| {
            crate::native::features::FeatureParameterUse {
                id: id.to_string(),
                operation_label: operation.to_string(),
                expression: expression.id.clone(),
                bindings: vec![format!("binding-{id}")],
                source_offsets: vec![source_offset],
            }
        };
        let uses = [
            parameter_use("later", "nx:feature-history:operation-label#0-1", 40),
            parameter_use("earlier", "nx:feature-history:operation-label#9-8", 30),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &[expression],
            &[],
            &uses,
            &mut annotations,
        );

        assert_eq!(
            ir.model.parameters[0].properties["parameter_use.0"],
            "earlier"
        );
        assert_eq!(
            ir.model.parameters[0].properties["parameter_use.1"],
            "later"
        );
    }

    #[test]
    fn nx_parameter_consumers_depend_on_preceding_expression_owner() {
        let expression = super::Expression {
            id: "nx:test:expression#20".to_string(),
            object_id: Some(20),
            record: None,
            declaration: None,
            name: "p20".to_string(),
            parameter_index: Some(20),
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: "5".to_string(),
            value: Some(5.0),
            source_entry: "part".to_string(),
            source_table: "table".to_string(),
            source_offset: 20,
        };
        let parameter_use = crate::native::features::FeatureParameterUse {
            id: "use".to_string(),
            operation_label: "nx:feature-history:operation-label#1-2".to_string(),
            expression: expression.id.clone(),
            bindings: vec!["binding".to_string()],
            source_offsets: vec![30],
        };
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &[expression],
            &[],
            std::slice::from_ref(&parameter_use),
            &mut annotations,
        );
        let parameter_owners = ir
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
            .collect();
        let dependencies = crate::native::attach::parameter_owner_dependencies(
            &parameter_owners,
            &[
                cadmpeg_ir::features::FeatureSourceContent::Parameter(
                    cadmpeg_ir::features::ParameterId("nx:test:parameter#20".into()),
                ),
                cadmpeg_ir::features::FeatureSourceContent::Parameter(
                    cadmpeg_ir::features::ParameterId("nx:test:parameter#20".into()),
                ),
            ],
        );

        assert_eq!(ir.model.features[0].ordinal, 0);
        assert_eq!(dependencies, [ir.model.parameters[0].owner.clone()]);
    }

    #[test]
    fn nx_feature_parameter_binding_joins_only_resolved_input_references() {
        use super::DataBlockReference;
        use crate::native::features::FeatureInputBlock;

        let input = FeatureInputBlock {
            id: "nx:feature-history:input-block#0-7-0".to_string(),
            operation_label: "nx:feature-history:operation-label#0-7".to_string(),
            input_slot: 0,
            object_index: 45,
            raw_object_index: vec![45],
            data_block: "nx:om-data-blocks-2:block#45".to_string(),
            source_offset: 700,
        };
        let reference = |ordinal: u32, declaration: Option<&str>| DataBlockReference {
            id: format!("nx:om-data-block-references-2-45:reference#{ordinal}"),
            data_block: input.data_block.clone(),
            ordinal,
            object_id: 201 + ordinal,
            raw_object_id: vec![0x80, (201 + ordinal) as u8],
            target_record: Some(format!("nx:om-record-directory-0:entry#{ordinal}")),
            target_expression_declaration: declaration.map(str::to_string),
            source_offset: 800 + u64::from(ordinal),
        };
        let references = [
            reference(0, Some("nx:om-expression-declarations-0:declaration#3")),
            reference(1, None),
        ];

        let expression = super::Expression {
            id: "nx:om-entry-9:expression#3".to_string(),
            object_id: Some(201),
            record: None,
            declaration: Some("nx:om-expression-declarations-0:declaration#3".to_string()),
            name: "p3".to_string(),
            parameter_index: Some(3),
            qualifier: None,
            unit: super::ExpressionUnit::Millimeter,
            expression: "12".to_string(),
            value: Some(12.0),
            source_entry: "/Root/UG_PART/UG_PART".to_string(),
            source_table: "table".to_string(),
            source_offset: 900,
        };
        let bindings = crate::native::features::feature_parameter_bindings(
            std::slice::from_ref(&input),
            &references,
            std::slice::from_ref(&expression),
        );
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[0].id,
            "nx:feature-history:parameter-binding#0-7-0-0"
        );
        assert_eq!(bindings[0].input_slot, 0);
        assert_eq!(bindings[0].reference_ordinal, 0);
        assert_eq!(bindings[0].object_id, 201);
        assert_eq!(
            bindings[0].expression_declaration,
            "nx:om-expression-declarations-0:declaration#3"
        );
        assert_eq!(
            bindings[0].expression.as_deref(),
            Some("nx:om-entry-9:expression#3")
        );

        let mut duplicate = expression.clone();
        duplicate.id = "nx:om-entry-9:expression#30".to_string();
        let ambiguous = crate::native::features::feature_parameter_bindings(
            &[input],
            &references,
            &[expression, duplicate],
        );
        assert_eq!(ambiguous.len(), 1);
        assert_eq!(ambiguous[0].expression, None);
    }

    #[test]
    fn om_offset_store_index_values_end_at_unique_aligned_product_record() {
        let mut bytes = vec![0, 0];
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&0x1020u32.to_le_bytes());
        bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
        assert_eq!(
            crate::om::offset_store_index_values(&bytes),
            Some((2, vec![7, 0x1020]))
        );

        let mut duplicate = bytes;
        duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
        assert!(crate::om::offset_store_index_values(&duplicate).is_none());
        assert_eq!(
            super::control_index_data_block(2, 700, 496).as_deref(),
            Some("nx:om-data-blocks-2:block#496")
        );
        assert!(super::control_index_data_block(2, 700, 700).is_none());
    }

    #[test]
    fn native_catalog_separates_offset_only_blocks_from_object_records() {
        let file =
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
        let container = container::scan_bytes(file).unwrap();

        assert!(super::object_records(&container).is_empty());
        let blocks = super::data_blocks(&container);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].block_ordinal, 0);
        assert_eq!(blocks[0].role, super::DataBlockRole::Control);
        assert_eq!(blocks[1].role, super::DataBlockRole::Column);
        assert!(blocks[0].byte_len > 0);
        let control_values = super::data_block_control_values(&container);
        assert_eq!(control_values.len(), 2);
        assert_eq!(control_values[0].data_block, blocks[0].id);
        assert_eq!(control_values[0].ordinal, 0);
        assert_eq!(control_values[0].value, 0);
        assert_eq!(control_values[1].value, 1);
        let classes = super::data_block_control_class_references(&container);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].data_block, blocks[0].id);
        assert_eq!(classes[0].ordinal, 0);
        assert_eq!(classes[0].class_ordinal, 0);
        assert_eq!(classes[0].class_name, "UGS::ModlFeature");
        assert_eq!(classes[0].class_definition, "nx:om-entry-0:class#8");
        assert!(super::string_values(&container).is_empty());
        assert!(super::object_references(&container).is_empty());
        let expressions = super::expressions(&container);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].object_id, None);
        assert_eq!(expressions[0].record, None);
    }

    #[test]
    fn native_abr_lane_resolves_nullable_slots_within_its_offset_store() {
        let mut store = offset_only_indexed_om_section();
        let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
        let end_at = index_start + 3 * 4;
        let end = u32::from_le_bytes(store[end_at..end_at + 4].try_into().unwrap()) as usize;
        let mut lane = vec![0x11, 0x02];
        lane.extend_from_slice(&[0xff; 15]);
        lane.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
        store.splice(end..end, lane.iter().copied());
        store[end_at..end_at + 4].copy_from_slice(&((end + lane.len()) as u32).to_le_bytes());
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
        let container = container::scan_bytes(file).unwrap();

        let lanes = super::data_block_abr_reference_lanes(&container);
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].slot_indices[0], Some(2));
        assert_eq!(
            lanes[0].slot_data_blocks[0].as_deref(),
            Some("nx:om-data-blocks-0:block#2")
        );
        assert!(lanes[0].slot_indices[1..].iter().all(Option::is_none));
        assert_eq!(lanes[0].slot_source_offsets.len(), 16);
        assert_eq!(lanes[0].slot_source_offsets[0], lanes[0].source_offset + 1);
    }

    #[test]
    fn om_numeric_expression_retains_formula_without_literal_value() {
        let text = b"(Number [mm]) p9: p2 * 2 + p7_radius; ";
        let mut bytes = b"hostglobalvariables".to_vec();
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text);
        bytes.push(0);

        let expressions = crate::om::numeric_expressions(&bytes);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].name, "p9");
        assert_eq!(expressions[0].expression, "p2 * 2 + p7_radius");
        assert_eq!(expressions[0].value, None);
        assert_eq!(
            super::expression_parameter_names(expressions[0].expression),
            vec!["p2", "p7_radius"]
        );
    }

    #[test]
    fn decode_retains_typed_nx_numeric_expression() {
        let mut cur = Cursor::new(prt_with_indexed_om_section());
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let expressions = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::Expression>("expressions")
            .unwrap();
        assert_eq!(result.ir.native.namespace("nx").unwrap().version, 155);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].object_id, Some(0x102));
        assert_eq!(expressions[0].parameter_index, Some(8));
        assert_eq!(
            expressions[0].qualifier.as_deref(),
            Some("CircularPattern_pattern_Circular_Dir_offset_angle")
        );
        assert_eq!(
            expressions[0].name,
            "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
        );
        assert_eq!(expressions[0].unit, super::ExpressionUnit::Degree);
        assert_eq!(expressions[0].expression, "120");
        assert_eq!(expressions[0].value, Some(120.0));
        assert_eq!(expressions[0].source_entry, "/Root/UG_PART/UG_PART");
        assert!(expressions[0]
            .source_table
            .starts_with("nx:om-entry-0:expression-table#"));
        let declarations = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ExpressionDeclaration>("expression_declarations")
            .unwrap();
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].object_id, 0x102);
        assert_eq!(declarations[0].parameter_index, 8);
        assert_eq!(declarations[0].literal.as_deref(), Some("120"));
        assert_eq!(
            expressions[0].declaration.as_deref(),
            Some(declarations[0].id.as_str())
        );
        let parameter = result
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == expressions[0].name)
            .unwrap();
        assert_eq!(
            parameter.properties.get("declaration"),
            Some(&declarations[0].id)
        );
        assert_eq!(
            parameter.properties.get("declaration_object_id"),
            Some(&"258".to_string())
        );
        let om_records = result
            .source_fidelity
            .retained_records
            .iter()
            .filter(|record| record.id.starts_with("nx:om-section-"))
            .collect::<Vec<_>>();
        assert_eq!(om_records.len(), 2);
        assert!(om_records.iter().all(|record| {
            record.data.as_ref().is_some_and(|data| {
                data.len() as u64 == record.byte_len
                    && cadmpeg_ir::hash::sha256_hex(data) == record.sha256
            })
        }));
        let object_records = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ObjectRecord>("object_records")
            .unwrap();
        assert_eq!(object_records.len(), 2);
        let headers = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::StoreHeader>("store_headers")
            .unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].version, "NX 2027.3102");
        assert_eq!(headers[0].object_id, Some(0x101));
        assert_eq!(object_records[1].object_id, Some(0x102));
        assert_eq!(
            object_records[1].object_id_source_offset,
            object_records[0]
                .object_id_source_offset
                .map(|offset| offset + 4)
        );
        assert_eq!(expressions[0].record.as_ref(), Some(&object_records[1].id));
        assert_eq!(object_records[1].record_ordinal, 1);
        assert_eq!(
            object_records[0].section_offset,
            object_records[1].section_offset
        );
        assert_eq!(object_records[1].byte_len, om_records[1].byte_len);
        assert_eq!(object_records[1].sha256, om_records[1].sha256);
        assert_eq!(
            object_records[1].dependencies,
            vec![object_records[0].id.clone()]
        );
        assert_eq!(
            object_records[0].dependents,
            vec![object_records[1].id.clone()]
        );
        let strings = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::StringValue>("string_values")
            .unwrap();
        assert_eq!(strings.len(), 1);
        assert_eq!(strings[0].record, object_records[1].id);
        assert_eq!(strings[0].object_id, Some(0x102));
        assert_eq!(strings[0].value, "SKETCH_001");
        let references = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ObjectReference>("object_references")
            .unwrap();
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].record, object_records[1].id);
        assert_eq!(references[0].object_id, Some(0x102));
        assert_eq!(references[0].value, 0x1234_5678);
        assert_eq!(references[0].target_record, None);
        assert_eq!(
            references[1].kind,
            super::ObjectReferenceKind::RecordOrdinal16
        );
        assert_eq!(references[1].value, 0);
        assert_eq!(
            references[1].target_record.as_ref(),
            Some(&object_records[0].id)
        );
        let handles = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::PersistentHandle>("persistent_handles")
            .unwrap();
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].value, 0x1234_5678);
        assert_eq!(handles[0].records, vec![object_records[1].id.clone()]);
        assert_eq!(handles[0].occurrence_count, 1);
        assert!(handles[0].external_records.is_empty());
        assert_eq!(result.ir.model.features.len(), 1);
        assert!(matches!(
            result.ir.model.features[0].definition,
            cadmpeg_ir::features::FeatureDefinition::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations,
                ..
            }
        ));
        assert_eq!(result.ir.model.features[0].suppressed, Some(false));
        assert_eq!(result.ir.model.parameters.len(), 1);
        assert_eq!(result.ir.model.parameters[0].expression, "120");
        let parameter = &result.ir.model.parameters[0];
        assert_eq!(parameter.name, expressions[0].name);
        assert!(matches!(
            parameter.value,
            Some(cadmpeg_ir::features::ParameterValue::Angle(
                cadmpeg_ir::features::Angle(value)
            )) if value == 120_f64.to_radians()
        ));
        assert_eq!(parameter.native_ref.as_ref(), Some(&expressions[0].id));
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    }

    #[test]
    fn nx_part_attributes_require_typed_atomic_xml() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
    <UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <Attribute owner="part" pdmBased="false" title="legacy" utf8title="Material"
        value="legacy-value" utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
    </UgAttributes>"#;
        let attributes = super::parse_part_attributes(xml, 7, "/Root/part/attrs", 100)
            .expect("typed attributes");
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].id, "nx:part-attributes-7:attribute#0");
        assert_eq!(attributes[0].title, "Material");
        assert_eq!(attributes[0].value, "Steel");
        assert_eq!(attributes[0].value_type, "StringAttributeType");
        assert!(!attributes[0].pdm_based);
        assert!(attributes[0].source_offset > 100);

        let mut terminated = xml.to_vec();
        terminated.push(0);
        assert_eq!(
            super::parse_part_attributes(&terminated, 7, "/Root/part/attrs", 100)
                .expect("terminated typed attributes"),
            attributes
        );
        terminated.push(0);
        assert!(super::parse_part_attributes(&terminated, 7, "/Root/part/attrs", 100).is_none());

        let malformed = xml
            .windows(b"pdmBased=\"false\"".len())
            .position(|window| window == b"pdmBased=\"false\"")
            .map(|at| {
                let mut malformed = xml.to_vec();
                malformed[at + b"pdmBased=\"".len()..at + b"pdmBased=\"false".len()]
                    .copy_from_slice(b"maybe");
                malformed
            })
            .unwrap();
        assert!(super::parse_part_attributes(&malformed, 7, "/Root/part/attrs", 100).is_none());
    }

    #[test]
    fn decode_retains_length_framed_nx_class_definition() {
        let mut cur = Cursor::new(prt_with_indexed_om_section());
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let classes = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ClassDefinition>("class_definitions")
            .unwrap();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "UGS::EXP_expression");
        assert_eq!(classes[0].ordinal, 0);
        assert_eq!(classes[0].trailing_code, 0x81);
        assert_eq!(classes[0].source_entry, "/Root/UG_PART/UG_PART");
    }

    #[test]
    fn decode_retains_length_framed_nx_field_definitions() {
        let mut cur = Cursor::new(prt_with_size_framed_om_section());
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let fields = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::FieldDefinition>("field_definitions")
            .unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "m_target");
        assert_eq!(fields[0].ordinal, 0);
        assert_eq!(fields[0].registry_suffix, [0x01, 0x02]);
        assert_eq!(fields[1].name, "m_tools");
        assert_eq!(fields[1].trailing_code, 0x81);
        assert!(fields[1].registry_suffix.is_empty());
        assert_eq!(fields[1].source_entry, "/Root/UG_PART/UG_PART");
        let classes = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ClassDefinition>("class_definitions")
            .unwrap();
        assert_eq!(classes[0].layout_prefix, &[0x81, 0x21]);
        assert_eq!(
            classes[0].schema_fingerprint,
            Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
        );
        assert_eq!(classes[0].layout_terminal, Some(0x06));
    }

    #[test]
    fn decode_retains_nx_arrangement_configurations() {
        let mut cur = Cursor::new(prt_with_arrangements());
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let configurations = result
            .ir
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::Configuration>("configurations")
            .unwrap();
        assert_eq!(configurations.len(), 2);
        assert_eq!(configurations[0].name, "Model");
        assert!(configurations[0].is_default);
        assert_eq!(configurations[1].name, "Exploded");
        assert!(!configurations[1].is_default);
        assert_eq!(result.ir.model.configurations.len(), 2);
        assert_eq!(result.ir.model.configurations[0].ordinal, 0);
        assert_eq!(result.ir.model.configurations[0].source_index, Some(0));
        assert_eq!(result.ir.model.configurations[0].name, "Model");
        assert!(result.ir.model.configurations[0].active);
        assert_eq!(
            result.ir.model.configurations[0].bodies.resolved(),
            Some(
                result
                    .ir
                    .model
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
                    .collect::<Vec<_>>()
                    .as_slice()
            )
        );
        assert_eq!(result.ir.model.configurations[1].ordinal, 1);
        assert_eq!(result.ir.model.configurations[1].name, "Exploded");
        assert!(!result.ir.model.configurations[1].active);
        assert!(result.ir.model.configurations[1].bodies.is_unresolved());
        let uses = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ConfigurationAttributeUse>("configuration_attribute_uses")
            .unwrap();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].configuration, configurations[0].id);
        assert_eq!(uses[0].name, "Model");
        assert_eq!(
            result.ir.model.configurations[0].properties["active_attribute_use"],
            uses[0].id
        );
        let attributes = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::PartAttribute>("part_attributes")
            .unwrap();
        let mut mismatch = attributes.clone();
        mismatch[0].value = "Other".to_string();
        assert!(super::configuration_attribute_uses(&configurations, &mismatch).is_empty());
        let mut duplicate = attributes.clone();
        duplicate.push(attributes[0].clone());
        assert!(super::configuration_attribute_uses(&configurations, &duplicate).is_empty());
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    }

    #[test]
    fn nx_neutral_active_configuration_requires_the_exact_attribute_join() {
        for active_name in [None, Some("Other")] {
            let mut cur = Cursor::new(prt_with_arrangement_attribute(active_name));
            let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
            let native = result
                .ir
                .native
                .namespace("nx")
                .unwrap()
                .arena_as::<super::Configuration>("configurations")
                .unwrap();
            assert!(native[0].is_default);
            assert!(
                result
                    .ir
                    .model
                    .configurations
                    .iter()
                    .all(|configuration| !configuration.active
                        && configuration.bodies.is_unresolved())
            );
        }
    }

    #[test]
    fn decode_retains_strict_tiff_material_texture_assets() {
        let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
        let malformed = [b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0];
        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/materialsTif/AISI Steel 4340", texture.to_vec()),
            ("/Root/materialsTif/Truncated", malformed.to_vec()),
        ]);

        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        let assets = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::MaterialTextureAsset>("material_texture_assets")
            .unwrap();

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "AISI Steel 4340");
        assert_eq!(assets[0].byte_order, "little_endian");
        assert_eq!(assets[0].version, 42);
        assert_eq!(assets[0].first_ifd_offset, 8);
        assert_eq!(assets[0].byte_len, texture.len() as u64);
        assert_eq!(assets[0].sha256, cadmpeg_ir::hash::sha256_hex(&texture));
        assert_eq!(assets[0].source_entry, "/Root/materialsTif/AISI Steel 4340");
    }

    #[test]
    fn decode_joins_qaf_material_names_to_texture_assets() {
        let texture = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0];
        let qaf = br#"<?xml version="1.0" encoding="UTF-8"?>
    <folderContents>
    <folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
    <folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
    </folderContents>"#;
        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/materialsTif/unmap$1", texture.to_vec()),
            ("/Root/qafmetadata", qaf.to_vec()),
        ]);

        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        let namespace = result.ir.native.namespace("nx").unwrap();
        let assets = namespace
            .arena_as::<super::MaterialTextureAsset>("material_texture_assets")
            .unwrap();
        let catalog = namespace
            .arena_as::<super::MaterialTextureCatalogEntry>("material_texture_catalog_entries")
            .unwrap();

        assert_eq!(assets.len(), 1);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].texture_asset, assets[0].id);
        assert_eq!(catalog[0].storage_path, "materialsTif/unmap$1");
        assert_eq!(
            catalog[0].material_path,
            "materialsTif/Carbon Fiber Harness Satin Coated"
        );
        assert_eq!(catalog[0].create_time, "2026-07-15T08:01:00");
        assert_eq!(catalog[0].modify_time, "2026-07-15T08:02:00");
        assert_eq!(catalog[0].source_entry, "/Root/qafmetadata");
    }

    #[test]
    fn decode_rejects_ambiguous_nx_arrangement_table_atomically() {
        for arrangements in [
            br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="YES" Name="Exploded"/></Arrangements>"#.as_slice(),
            br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Model"/></Arrangements>"#.as_slice(),
        ] {
            let file = prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/part/arrangements", arrangements.to_vec()),
            ]);
            let mut cur = Cursor::new(file);
            let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
            assert!(result.ir.native.namespace("nx").is_none_or(|namespace| {
                namespace
                    .arena_as::<super::Configuration>("configurations")
                    .unwrap()
                    .is_empty()
            }));
            assert!(result.ir.model.configurations.is_empty());
        }
    }

    #[test]
    fn decode_rejects_duplicate_nx_configuration_stream_paths_atomically() {
        let arrangements =
            br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
        let attributes = br#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="Model" version="3" type="StringAttributeType"/></UgAttributes>"#.to_vec();
        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/part/arrangements", arrangements.clone()),
            ("/Root/part/arrangements", arrangements.clone()),
            ("/Root/part/attrs", attributes.clone()),
        ]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        assert!(result.ir.model.configurations.is_empty());

        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/part/arrangements", arrangements),
            ("/Root/part/attrs", attributes.clone()),
            ("/Root/part/attrs", attributes),
        ]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir.model.configurations.len(), 1);
        assert!(!result.ir.model.configurations[0].active);
        assert!(result.ir.model.configurations[0].bodies.is_unresolved());
        assert!(result.ir.native.namespace("nx").is_none_or(|namespace| {
            namespace
                .arena_as::<super::PartAttribute>("part_attributes")
                .unwrap()
                .is_empty()
        }));
    }

    #[test]
    fn assembly_metadata_lists_external_child_paths() {
        let mut cur = Cursor::new(assembly_with_external_paths());
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let attrs = &result.ir.source.expect("source").attributes;
        assert_eq!(
            attrs.get("external_reference.0").map(String::as_str),
            Some("child.prt")
        );
        assert_eq!(
            attrs.get("external_reference.1").map(String::as_str),
            Some("nested/b.prt")
        );
        let references = result
            .ir
            .native
            .namespace("nx")
            .expect("NX native namespace")
            .arena_as::<super::ExternalReference>("external_references")
            .expect("typed external references");
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].ordinal, 0);
        assert_eq!(references[0].path, "child.prt");
        assert_eq!(references[1].ordinal, 1);
        assert_eq!(references[1].path, "nested/b.prt");
        assert!(references[0].source_offset < references[1].source_offset);
    }

    #[test]
    fn persistent_handle_identity_bridges_om_and_external_records() {
        let reference = super::ObjectReference {
            id: "nx:test:reference#0".into(),
            record: "nx:test:om-record#0".into(),
            object_id: Some(1),
            ordinal: 0,
            kind: super::ObjectReferenceKind::PersistentHandle,
            value: 0x1020_3040,
            target_record: None,
            source_entry: "om".into(),
            source_offset: 0,
        };
        let external = super::ExternalReferenceRecord {
            id: "nx:test:external-record#6".into(),
            record_id: 6,
            declared_count: 1,
            id_slots: [0; 4],
            handles: vec![0x1020_3040],
            closing_duplicate: true,
            prefix_byte_len: 31,
            tail_byte_len: 0,
            source_entry: "external".into(),
            source_offset: 10,
        };
        let control = super::DataBlockControlReference {
            id: "nx:test:control-reference#0".into(),
            data_block: "nx:test:control-block#0".into(),
            ordinal: 0,
            kind: super::ObjectReferenceKind::PersistentHandle,
            value: 0x1020_3040,
            source_offset: 20,
        };

        let tail_pair = super::ExternalReferenceTailReferencePair {
            id: "nx:test:tail-pair#0".into(),
            handle_set_record: external.id.clone(),
            ordinal: 0,
            persistent_handle: 0x5060_7080,
            tagged_reference: 7,
            source_offset: 30,
        };

        let handles =
            super::persistent_handles(&[reference], &[control], &[external], &[tail_pair]);

        assert_eq!(handles.len(), 2);
        assert_eq!(handles[0].records, ["nx:test:om-record#0"]);
        assert_eq!(handles[0].occurrence_count, 2);
        assert_eq!(handles[0].data_blocks, ["nx:test:control-block#0"]);
        assert_eq!(handles[0].external_records, ["nx:test:external-record#6"]);
        assert_eq!(handles[0].external_occurrence_count, 2);
        assert_eq!(handles[1].value, 0x5060_7080);
        assert_eq!(handles[1].external_records, ["nx:test:external-record#6"]);
        assert_eq!(handles[1].external_occurrence_count, 1);
    }

    #[test]
    fn nx_control_handle_pairs_require_maximal_runs_of_exactly_two() {
        let reference = |ordinal: u32, offset: u64| super::DataBlockControlReference {
            id: format!("reference#{ordinal}"),
            data_block: "block#0".into(),
            ordinal,
            kind: super::ObjectReferenceKind::PersistentHandle,
            value: ordinal + 100,
            source_offset: offset,
        };
        let references = [
            reference(0, 10),
            reference(1, 15),
            reference(2, 30),
            reference(3, 35),
            reference(4, 40),
        ];
        let pairs = super::data_block_control_handle_pairs(&references);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].id, "nx:om-data-block-control:handle-pair#10");
        assert_eq!(pairs[0].first_reference, "reference#0");
        assert_eq!(pairs[0].second_reference, "reference#1");
        assert_eq!(pairs[0].first_handle, 100);
        assert_eq!(pairs[0].second_handle, 101);
    }

    #[test]
    fn native_retains_rmfastload_table_and_member_words() {
        let container = container::scan_bytes(rmfastload_prt()).unwrap();
        let entry_offset = container
            .entries
            .iter()
            .find(|entry| entry.name == "/Root/FastLoad/RMFastLoad")
            .and_then(|entry| entry.file_span)
            .expect("RMFastLoad span")
            .0;
        let (table, object_ids) =
            super::rmfastload_object_id_table(&container).expect("native RMFastLoad table");

        assert_eq!(table.id, "nx:rmfastload:object-id-table#0");
        assert_eq!(table.members.len(), 50);
        assert_eq!(table.raw_count, 50u32.to_le_bytes());
        assert_eq!(table.registry_source_offset, entry_offset);
        assert_eq!(
            table.source_offset,
            entry_offset + b"UGS::Solid::Topol".len() as u64
        );
        assert_eq!(object_ids[0].table, table.id);
        assert_eq!(object_ids[0].value, 1);
        assert_eq!(object_ids[0].raw, 1u32.to_le_bytes());
        assert_eq!(object_ids[0].source_offset, table.source_offset + 4);
        assert_eq!(object_ids[49].ordinal, 49);
        assert_eq!(object_ids[49].value, 50);
        assert_eq!(object_ids[49].raw, 50u32.to_le_bytes());
        assert_eq!(table.members[49], object_ids[49].id);
    }

    #[test]
    fn decode_selects_dominant_rmfastload_body() {
        let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let namespace = result.ir.native.namespace("nx").expect("NX namespace");
        let tables = namespace
            .arena_as::<super::RmFastLoadObjectIdTable>("rmfastload_object_id_tables")
            .expect("RMFastLoad tables");
        let object_ids = namespace
            .arena_as::<super::RmFastLoadObjectId>("rmfastload_object_ids")
            .expect("RMFastLoad object IDs");

        assert_eq!(result.ir.model.bodies.len(), 1);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].members.len(), 50);
        assert_eq!(object_ids.len(), 50);
        assert_eq!(object_ids[0].value, 1_000);
        assert_eq!(object_ids[49].value, 1_049);
        assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
        assert_eq!(result.ir.model.faces.len(), 50);
        assert_eq!(result.ir.model.surfaces.len(), 50);
        assert!(result
            .ir
            .model
            .faces
            .iter()
            .all(|face| face.id.0.starts_with("nx:s0:")));
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .all(|surface| surface.id.0.starts_with("nx:s0:")));
        assert_eq!(
            result
                .ir
                .source
                .as_ref()
                .and_then(|source| source.attributes.get("active_body_selector"))
                .map(String::as_str),
            Some("rmfastload_object_id_membership")
        );
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(
            validation.findings.is_empty(),
            "findings: {:?}",
            validation.findings
        );
    }

    #[test]
    fn data_block_column_index_tables_require_complete_mode_and_target_sequence() {
        use super::{
            data_block_column_index_tables, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
        };

        let linked = |id: &str, target: u32, mode: u8, offset: u64| DataBlockLinkedIndexRow {
            id: id.into(),
            section_ordinal: 2,
            ordinal: 0,
            first_index: 20,
            raw_first_index: vec![20],
            discriminator: 0x16,
            target_index: target,
            raw_target_index: vec![target as u8],
            indices: [5, 6, 7],
            raw_indices: [vec![5], vec![6], vec![7]],
            data_blocks: [
                format!("block#{target}"),
                "block#5".into(),
                "block#6".into(),
                "block#7".into(),
            ],
            flag: 3,
            mode,
            source_entry: "entry".into(),
            opening_data_block: format!("opening-block-{id}"),
            opening_block_offset: 8,
            source_offset: offset,
            first_index_source_offset: offset + 2,
            target_index_source_offset: offset + 7,
            index_source_offsets: [offset + 12, offset + 13, offset + 14],
        };
        let target = |id: &str, index: u32, mode: u8, offset: u64| DataBlockTargetIndexRow {
            id: id.into(),
            section_ordinal: 2,
            ordinal: 0,
            target_index: index,
            raw_target_index: vec![index as u8],
            indices: [5, 6, 7],
            raw_indices: [vec![5], vec![6], vec![7]],
            data_blocks: [
                format!("block#{index}"),
                "block#5".into(),
                "block#6".into(),
                "block#7".into(),
            ],
            mode,
            source_entry: "entry".into(),
            opening_data_block: format!("opening-block-{id}"),
            opening_block_offset: 8,
            source_offset: offset,
            target_index_source_offset: offset + 5,
            index_source_offsets: [offset + 10, offset + 11, offset + 12],
        };
        let linked_rows = [
            linked("opening", 63, 7, 100),
            linked("linked-59", 59, 4, 200),
            linked("linked-58", 58, 4, 225),
        ];
        let target_rows = [
            target("target-62", 62, 7, 125),
            target("target-61", 61, 7, 150),
            target("target-60", 60, 4, 175),
        ];

        let tables = data_block_column_index_tables(&linked_rows, &target_rows);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].id, "nx:om-data-block-column-index-tables:table#2");
        assert_eq!(tables[0].opening_linked_row, "opening");
        assert_eq!(
            tables[0].target_rows,
            ["target-62", "target-61", "target-60"]
        );
        assert_eq!(tables[0].linked_rows, ["linked-59", "linked-58"]);
        assert_eq!(tables[0].first_target_index, 63);
        assert_eq!(tables[0].last_target_index, 58);
        assert_eq!(tables[0].source_offset, 100);

        let mut gap = target_rows.clone();
        gap[1].target_index = 60;
        assert!(data_block_column_index_tables(&linked_rows, &gap).is_empty());
        let mut incomplete_mode = target_rows.clone();
        incomplete_mode[2].mode = 7;
        assert!(data_block_column_index_tables(&linked_rows, &incomplete_mode).is_empty());
    }

    #[test]
    fn external_reference_record_slots_resolve_atomically_in_the_same_stream() {
        use super::{
            external_reference_record_children, external_reference_record_string_uses,
            ExternalReference, ExternalReferenceRecord,
        };

        let references = (0..4)
            .map(|ordinal| ExternalReference {
                id: format!("reference#{ordinal}"),
                ordinal,
                path: format!("value-{ordinal}"),
                source_entry: "stream".into(),
                source_offset: 100 + u64::from(ordinal),
            })
            .collect::<Vec<_>>();
        let record = ExternalReferenceRecord {
            id: "record#7".into(),
            record_id: 7,
            declared_count: 2,
            id_slots: [0, 3, 1, 2],
            handles: vec![10, 20],
            closing_duplicate: true,
            prefix_byte_len: 40,
            tail_byte_len: 5,
            source_entry: "stream".into(),
            source_offset: 20,
        };
        let uses = external_reference_record_string_uses(&[record.clone()], &references);
        assert_eq!(uses.len(), 4);
        assert_eq!(uses[0].id, "nx:external-reference:record-string-use#7-0");
        assert_eq!(
            uses.iter().map(|use_| use_.slot).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            uses.iter()
                .map(|use_| use_.string_index)
                .collect::<Vec<_>>(),
            [0, 3, 1, 2]
        );
        assert_eq!(uses[1].external_reference, "reference#3");
        assert_eq!(uses[1].source_offset, 31);
        let mut child_references = references.clone();
        child_references[0].path = "child.prt".into();
        let child_uses =
            external_reference_record_string_uses(&[record.clone()], &child_references);
        let children =
            external_reference_record_children(&[record.clone()], &child_references, &child_uses);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].external_record, record.id);
        assert_eq!(children[0].name_reference, "reference#0");
        assert_eq!(children[0].directory_reference, "reference#1");
        assert!(
            external_reference_record_children(&[record.clone()], &references, &uses).is_empty()
        );

        let mut out_of_range = record.clone();
        out_of_range.id_slots[2] = 4;
        assert!(external_reference_record_string_uses(&[out_of_range], &references).is_empty());
        let mut duplicate = references.clone();
        duplicate.push(references[0].clone());
        assert!(external_reference_record_string_uses(&[record], &duplicate).is_empty());
    }
}
