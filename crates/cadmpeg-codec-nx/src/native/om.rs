// SPDX-License-Identifier: Apache-2.0
//! Object-model, data-block, expression, and external-reference extractors and record types.

#[allow(clippy::wildcard_imports)]
use super::*;

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
