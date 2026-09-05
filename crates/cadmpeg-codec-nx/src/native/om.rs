// SPDX-License-Identifier: Apache-2.0
//! Object-model, data-block, expression, and external-reference extractors and record types.

#[allow(clippy::wildcard_imports)]
use super::*;

use cadmpeg_core::decode::View;

use crate::native::segments::segment_om_links;
use crate::om::{IndexedStore, TypeDefinition as OmTypeDefinition};

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
    /// Audit model declaring only `UGS::OM::SaveAuditTrail`.
    AuditTrail,
    /// More than one specialized role marker occurs in the registry.
    Ambiguous,
    /// No specialized or audit role marker occurs in the registry.
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

/// One complete row retained from an audit-trail record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmAuditTrailRow {
    /// Globally unique audit-row identity.
    pub id: String,
    /// Owning audit-trail section link.
    pub section_link: String,
    /// Monotone row ordinal.
    pub ordinal: u32,
    /// Exact serialized ordinal token.
    pub raw_ordinal: Vec<u8>,
    /// Optional selector in the `04 05 selector 00` envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_selector: Option<u8>,
    /// Big-endian row timestamp.
    pub timestamp: u32,
    /// Tagged row-value marker.
    pub value_marker: u8,
    /// Decoded tagged row value.
    pub value: u32,
    /// Exact serialized tagged row value.
    pub raw_value: Vec<u8>,
    /// Exact complete row bytes.
    pub raw: Vec<u8>,
    /// Directory entry containing the audit-trail section.
    pub source_entry: String,
    /// Absolute file offset of the row's opening `04` marker.
    pub source_offset: u64,
    /// Absolute exclusive end offset after the tagged value.
    pub end_offset: u64,
}

/// One row from the feature-history state journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateJournalRow {
    /// Big-endian Unix timestamp stored by the journal.
    pub timestamp: u32,
    /// Tagged schema-value marker.
    pub value_marker: u8,
    /// Decoded tagged schema value.
    pub value: u32,
    /// Exact tagged schema-value token.
    pub raw_value: Vec<u8>,
    /// Journal schema identifier.
    pub schema_id: u32,
    /// Exact schema-identifier token.
    pub raw_schema_id: Vec<u8>,
    /// Monotone state-counter ordinal.
    pub state_ordinal: u32,
    /// Exact state-ordinal token.
    pub raw_state_ordinal: Vec<u8>,
    /// Absolute file offset of the row's `e0` marker.
    pub source_offset: u64,
    /// Absolute exclusive end offset after the `13` terminator.
    pub end_offset: u64,
}

/// One anchored state-journal group from a feature-history section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateJournalGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based group ordinal in serialized order.
    pub ordinal: u32,
    /// Exact two-byte group selector.
    pub selector: [u8; 2],
    /// Ordered journal rows.
    pub rows: Vec<OmOperationStateJournalRow>,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the `04` group opener.
    pub source_offset: u64,
    /// Absolute exclusive end offset after the final row.
    pub end_offset: u64,
}

/// One row from the feature-history operation-state counter map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateCounter {
    /// Globally unique counter-row identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based row ordinal within the section's counter map.
    pub ordinal: u32,
    /// Serialized counter-row kind (`01` or `02`).
    pub row_kind: u8,
    /// Object carrying the introduced/last-modified state pair.
    pub object_index: u32,
    /// Exact serialized object-index token.
    pub raw_object_index: Vec<u8>,
    /// State-journal ordinal at object introduction.
    pub introduced_state: u8,
    /// State-journal ordinal at the object's last modification.
    pub modified_state: u8,
    /// Absolute file offset of the object-index token.
    pub object_index_source_offset: u64,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the row's `05` marker.
    pub source_offset: u64,
}

/// One typed member in an `m_rollForwardStates` group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OmRollForwardStateRow {
    /// Ordered feature-record member from a `4a` list row.
    List {
        /// Zero-based position within the group list.
        ordinal: u32,
        /// Ordered feature-history object index.
        object_index: u32,
        /// Exact serialized object-index token.
        raw_object_index: Vec<u8>,
        /// Serialized list position.
        position: u32,
        /// Exact serialized position token.
        raw_position: Vec<u8>,
        /// Absolute file offset of the `4a` row marker.
        source_offset: u64,
    },
    /// Relation member from a `4f` or `48` pair row.
    Pair {
        /// Zero-based position within the group row list.
        ordinal: u32,
        /// Schema-generation relation tag.
        tag: u8,
        /// First relation endpoint.
        first: u32,
        /// Exact serialized first endpoint token.
        raw_first: Vec<u8>,
        /// Second relation endpoint.
        second: u32,
        /// Exact serialized second endpoint token.
        raw_second: Vec<u8>,
        /// Absolute file offset of the relation tag.
        source_offset: u64,
    },
}

/// One counted `m_rollForwardStates` group from a feature-history section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmRollForwardStateGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based group ordinal within the table.
    pub ordinal: u32,
    /// Exact two-byte group opener.
    pub opener: [u8; 2],
    /// Whether the count used the nonempty `01 count` form.
    pub count_prefix: Option<u8>,
    /// Serialized member count including the implicit owner slot.
    pub declared_count: u8,
    /// Ordered typed rows in the group.
    pub rows: Vec<OmRollForwardStateRow>,
    /// Exact bytes between the final group and the counter-map boundary.
    pub table_trailing_bytes: Vec<u8>,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the group opener.
    pub source_offset: u64,
    /// Absolute file offset of the counter-map boundary.
    pub table_end_offset: u64,
}

/// Typed high-byte outcome of an operation-state diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmOperationStateMessageSeverity {
    /// Non-fatal update alert.
    Alert,
    /// Failed update outcome.
    Failure,
}

fn operation_state_message_severity(word: u16) -> Option<OmOperationStateMessageSeverity> {
    match word >> 8 {
        0x01 => Some(OmOperationStateMessageSeverity::Alert),
        0x03 => Some(OmOperationStateMessageSeverity::Failure),
        _ => None,
    }
}

/// One standalone operation-state message record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateMessage {
    /// Globally unique message identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based message ordinal within the bounded state block.
    pub ordinal: u32,
    /// Serialized length byte.
    pub declared_length: u8,
    /// Exact Part Navigator diagnostic text.
    pub text: String,
    /// Tagged value marker following the four zero bytes.
    pub value_marker: u8,
    /// Decoded tagged value.
    pub value: u32,
    /// Exact serialized tagged value.
    pub raw_value: Vec<u8>,
    /// Big-endian count or severity word.
    pub count_or_severity: u16,
    /// Typed high-byte severity when the word uses a known outcome class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<OmOperationStateMessageSeverity>,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the opening `03` marker.
    pub source_offset: u64,
}

/// Native payload retained by one operation-state status row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OmOperationStateStatusPayload {
    /// Normal built/healthy state marker.
    Plain,
    /// Status carrying one linked object index.
    Linked {
        /// Serialized link discriminator.
        link_code: u8,
        /// Linked object index.
        object_index: u32,
        /// Exact linked object-index token.
        raw_object_index: Vec<u8>,
    },
    /// Status carrying an inline diagnostic message.
    Diagnostic {
        /// Serialized message length byte.
        declared_length: u8,
        /// Exact diagnostic text.
        text: String,
        /// Tagged value marker.
        value_marker: u8,
        /// Decoded tagged value.
        value: u32,
        /// Exact tagged value token.
        raw_value: Vec<u8>,
        /// Big-endian count or severity word.
        count_or_severity: u16,
        /// Typed high-byte severity when the word uses a known outcome class.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<OmOperationStateMessageSeverity>,
    },
    /// Typed status whose payload grammar is not assigned.
    Opaque {
        /// Exact bounded payload bytes.
        raw: Vec<u8>,
    },
}

/// One per-object operation-state status row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateStatus {
    /// Globally unique status-row identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based row ordinal within the status table.
    pub ordinal: u32,
    /// Decoded non-null status-code value.
    pub status_code: u32,
    /// Exact serialized status-code token.
    pub raw_status_code: Vec<u8>,
    /// Decoded non-null object carrying the status.
    pub object_index: u32,
    /// Exact serialized object-index token.
    pub raw_object_index: Vec<u8>,
    /// Exact typed status payload.
    pub payload: OmOperationStateStatusPayload,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the status-code token.
    pub source_offset: u64,
    /// Absolute exclusive end offset of the row.
    pub end_offset: u64,
}

/// One serialized feature-record slot in an operation-state slot lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateSlot {
    /// Zero-based slot ordinal.
    pub ordinal: u32,
    /// Decoded object index; null slots remain null.
    pub object_index: Option<u32>,
    /// Exact serialized object-index token.
    pub raw_object_index: Vec<u8>,
}

/// One `02 01 11 ... 02 11` operation-state slot lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmOperationStateSlotLane {
    /// Globally unique slot-lane identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based lane ordinal within the status table.
    pub ordinal: u32,
    /// Ordered null or object-index slots.
    pub slots: Vec<OmOperationStateSlot>,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the lane prefix.
    pub source_offset: u64,
    /// Absolute exclusive end offset after the lane terminator.
    pub end_offset: u64,
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

/// Decode complete rows from audit-trail record areas.
pub fn audit_trail_rows(container: &Container) -> Vec<OmAuditTrailRow> {
    let sections = container.om_sections();
    segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::AuditTrail)
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(rows) = section.audit_trail_rows() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            rows.into_iter()
                .filter_map(move |row| {
                    let ordinal = row.ordinal.value?;
                    Some(OmAuditTrailRow {
                        id: format!("nx:audit-trail:row#{section_key}-{ordinal:010}"),
                        section_link: link.id.clone(),
                        ordinal,
                        raw_ordinal: row.ordinal.raw.to_vec(),
                        frame_selector: row.frame_selector,
                        timestamp: row.timestamp,
                        value_marker: row.value.marker,
                        value: row.value.value,
                        raw_value: row.value.raw.to_vec(),
                        raw: row.raw.to_vec(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + row.offset as u64,
                        end_offset: entry_offset + row.end_offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact object state-counter rows from canonical feature-history areas.
pub fn operation_state_counters(container: &Container) -> Vec<OmOperationStateCounter> {
    let sections = container.om_sections();
    crate::native::features::canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(map) = section.operation_state_counter_map() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            map.rows
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, row)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    Some(OmOperationStateCounter {
                        id: format!(
                            "nx:feature-history:operation-state-counter#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        row_kind: row.row_kind,
                        object_index: row.object_index.value?,
                        raw_object_index: row.object_index.raw.to_vec(),
                        introduced_state: row.introduced_state,
                        modified_state: row.modified_state,
                        object_index_source_offset: entry_offset + row.object_index.offset as u64,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + row.offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode anchored state-journal groups from canonical feature-history areas.
pub fn operation_state_journal_groups(container: &Container) -> Vec<OmOperationStateJournalGroup> {
    let sections = container.om_sections();
    crate::native::features::canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(groups) = section.operation_state_journal_groups() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            groups
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, group)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    let rows = group
                        .rows
                        .into_iter()
                        .map(|row| {
                            Some(OmOperationStateJournalRow {
                                timestamp: row.timestamp,
                                value_marker: row.value.marker,
                                value: row.value.value,
                                raw_value: row.value.raw.to_vec(),
                                schema_id: row.schema_id.value?,
                                raw_schema_id: row.schema_id.raw.to_vec(),
                                state_ordinal: row.ordinal.value?,
                                raw_state_ordinal: row.ordinal.raw.to_vec(),
                                source_offset: entry_offset + row.offset as u64,
                                end_offset: entry_offset + row.end_offset as u64,
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(OmOperationStateJournalGroup {
                        id: format!(
                            "nx:feature-history:operation-state-journal-group#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        selector: group.selector,
                        rows,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + group.offset as u64,
                        end_offset: entry_offset + group.end_offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode field-declared roll-forward groups from canonical feature-history areas.
pub fn operation_state_groups(container: &Container) -> Vec<OmRollForwardStateGroup> {
    let sections = container.om_sections();
    crate::native::features::canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(table) = section.operation_state_group_table() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            table
                .groups
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, group)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    let rows = group
                        .rows
                        .into_iter()
                        .enumerate()
                        .filter_map(|(row_ordinal, row)| {
                            let ordinal = u32::try_from(row_ordinal).ok()?;
                            match row {
                                crate::om::OperationStateGroupRow::List {
                                    offset,
                                    object_index,
                                    position,
                                } => Some(OmRollForwardStateRow::List {
                                    ordinal,
                                    object_index: object_index.value?,
                                    raw_object_index: object_index.raw.to_vec(),
                                    position: position.value?,
                                    raw_position: position.raw.to_vec(),
                                    source_offset: entry_offset + offset as u64,
                                }),
                                crate::om::OperationStateGroupRow::Pair {
                                    offset,
                                    tag,
                                    first,
                                    second,
                                } => Some(OmRollForwardStateRow::Pair {
                                    ordinal,
                                    tag,
                                    first: first.value?,
                                    raw_first: first.raw.to_vec(),
                                    second: second.value?,
                                    raw_second: second.raw.to_vec(),
                                    source_offset: entry_offset + offset as u64,
                                }),
                            }
                        })
                        .collect();
                    Some(OmRollForwardStateGroup {
                        id: format!(
                            "nx:feature-history:roll-forward-state-group#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        opener: group.opener,
                        count_prefix: group.count_prefix,
                        declared_count: group.declared_count,
                        rows,
                        table_trailing_bytes: table.trailing_bytes.to_vec(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + group.offset as u64,
                        table_end_offset: entry_offset + table.end_offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode standalone operation-state messages from canonical feature-history areas.
pub fn operation_state_messages(container: &Container) -> Vec<OmOperationStateMessage> {
    let sections = container.om_sections();
    crate::native::features::canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(messages) = section.operation_state_messages() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            messages
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, message)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    Some(OmOperationStateMessage {
                        id: format!(
                            "nx:feature-history:operation-state-message#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        declared_length: message.declared_length,
                        text: message.text.to_string(),
                        value_marker: message.value.marker,
                        value: message.value.value,
                        raw_value: message.value.raw.to_vec(),
                        count_or_severity: message.count_or_severity,
                        severity: operation_state_message_severity(message.count_or_severity),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + message.offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact per-object operation-state status rows from feature-history areas.
pub fn operation_state_statuses(container: &Container) -> Vec<OmOperationStateStatus> {
    let sections = container.om_sections();
    crate::native::features::canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(table) = section.operation_state_status_table() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            table
                .rows
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, row)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    let status_code = row.status_code.value?;
                    let object_index = row.object_index.value?;
                    let payload = match row.payload {
                        crate::om::OperationStateStatusPayload::Plain => {
                            OmOperationStateStatusPayload::Plain
                        }
                        crate::om::OperationStateStatusPayload::Linked {
                            link_code,
                            object_index,
                        } => OmOperationStateStatusPayload::Linked {
                            link_code,
                            object_index: object_index.value?,
                            raw_object_index: object_index.raw.to_vec(),
                        },
                        crate::om::OperationStateStatusPayload::Diagnostic { message } => {
                            OmOperationStateStatusPayload::Diagnostic {
                                declared_length: message.declared_length,
                                text: message.text.to_string(),
                                value_marker: message.value.marker,
                                value: message.value.value,
                                raw_value: message.value.raw.to_vec(),
                                count_or_severity: message.count_or_severity,
                                severity: operation_state_message_severity(
                                    message.count_or_severity,
                                ),
                            }
                        }
                        crate::om::OperationStateStatusPayload::Opaque { raw } => {
                            OmOperationStateStatusPayload::Opaque { raw: raw.to_vec() }
                        }
                    };
                    Some(OmOperationStateStatus {
                        id: format!(
                            "nx:feature-history:operation-state-status#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        status_code,
                        raw_status_code: row.status_code.raw.to_vec(),
                        object_index,
                        raw_object_index: row.object_index.raw.to_vec(),
                        payload,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + row.offset as u64,
                        end_offset: entry_offset + row.end_offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact feature-record slot lanes from feature-history status blocks.
pub fn operation_state_slot_lanes(container: &Container) -> Vec<OmOperationStateSlotLane> {
    let sections = container.om_sections();
    crate::native::features::canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, link)| {
            let Some((entry, section)) = sections.iter().find(|(entry, section)| {
                entry
                    .file_span
                    .map_or(section.offset as u64, |(offset, _)| {
                        offset + section.offset as u64
                    })
                    == link.section_offset
            }) else {
                return Vec::new();
            };
            let Some(table) = section.operation_state_status_table() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_key = format!("{section_ordinal:010}");
            table
                .slot_lanes
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, lane)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    let slots = lane
                        .slots
                        .into_iter()
                        .enumerate()
                        .map(|(slot_ordinal, slot)| OmOperationStateSlot {
                            ordinal: u32::try_from(slot_ordinal).expect("slot ordinal fits u32"),
                            object_index: slot.value,
                            raw_object_index: slot.raw.to_vec(),
                        })
                        .collect();
                    Some(OmOperationStateSlotLane {
                        id: format!(
                            "nx:feature-history:operation-state-slot-lane#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        slots,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + lane.offset as u64,
                        end_offset: entry_offset + lane.end_offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Unit declared by an NX numeric expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionUnit {
    /// Model length in millimeters as stored by NX.
    Millimeter,
    /// Model length in inches as stored by NX.
    Inch,
    /// Angular value in degrees as stored by NX.
    Degree,
    /// Unit label without a neutral dimensional mapping.
    Native(String),
}

const INCH_TO_MILLIMETERS: f64 = 25.4;

impl ExpressionUnit {
    pub(crate) fn property_name(&self) -> String {
        match self {
            Self::Millimeter => "millimeter".to_string(),
            Self::Inch => "inch".to_string(),
            Self::Degree => "degree".to_string(),
            Self::Native(unit) => unit.clone(),
        }
    }
}

pub(crate) fn expression_length_in_millimeters(unit: &ExpressionUnit, value: f64) -> Option<f64> {
    match unit {
        ExpressionUnit::Millimeter => Some(value),
        ExpressionUnit::Inch => Some(value * INCH_TO_MILLIMETERS),
        ExpressionUnit::Degree | ExpressionUnit::Native(_) => None,
    }
}

pub(crate) fn canonical_expression_value(unit: &str, value: f64) -> Option<f64> {
    match unit {
        "millimeter" => Some(value),
        "inch" => Some(value * INCH_TO_MILLIMETERS),
        "degree" => Some(value.to_radians()),
        _ => None,
    }
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

pub(crate) fn evaluate_parameterized_expression(
    expression: &str,
    mut parameter_value: impl FnMut(&str) -> Option<f64>,
) -> Option<f64> {
    let bytes = expression.as_bytes();
    let mut substituted = String::with_capacity(expression.len());
    let mut at = 0usize;
    while at < bytes.len() {
        if let Some(end) = expression_parameter_reference_end(bytes, at) {
            let value = parameter_value(&expression[at..end])?;
            substituted.push('(');
            substituted.push_str(&value.to_string());
            substituted.push(')');
            at = end;
        } else {
            substituted.push(char::from(bytes[at]));
            at += 1;
        }
    }
    crate::om::evaluate_constant_expression(&substituted)
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
    /// First registry-token byte serialized after the class name (legacy field name).
    pub trailing_code: u8,
    /// Decoded storage token from the complete class registry tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_storage_code: Option<u32>,
    /// One-based base-class ordinal from the complete class registry tail.
    /// Zero denotes the registry root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_base_class: Option<u32>,
    /// One-based reference-list ordinal from the complete class registry tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_reference: Option<u32>,
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
    /// First registry-token byte serialized immediately after the name (legacy field name).
    pub trailing_code: u8,
    /// Decoded storage token from the complete member registry head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_storage_code: Option<u32>,
    /// One-based declaring-class ordinal from the complete member registry head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_owner_class: Option<u32>,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_prefix: Vec<u8>,
    /// Stable eight-byte field fingerprint in a framed registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_fingerprint: Option<[u8; 8]>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_terminal: Option<u8>,
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
    /// Content-backed identity when the scoped exact bytes are unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_identity: Option<String>,
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

/// Return a content-backed identity for one indexed OM object record.
///
/// The source entry scopes the exact bytes. Callers must only admit the value
/// when this key is unique in that scope; equal records have no stable
/// position-independent identity without another serialized owner.
pub(crate) fn stable_object_record_identity(source_entry: &str, bytes: &[u8]) -> String {
    let mut seed = Vec::with_capacity(source_entry.len() + bytes.len() + 20);
    seed.extend_from_slice(b"nx:om:object-record\0");
    seed.extend_from_slice(source_entry.as_bytes());
    seed.push(0);
    seed.extend_from_slice(bytes);
    format!(
        "nx:om:object-record:{}",
        cadmpeg_ir::hash::sha256_hex(&seed)
    )
}

/// Return position-independent identities for one indexed object-record graph.
///
/// `RecordOrdinal16` values are local to one indexed section. The canonical
/// graph replaces those values with links to records in traversal order, so a
/// directory reorder does not change the identity. The traversal is explicit
/// rather than recursive because malformed or adversarial records must not
/// turn identity extraction into a stack overflow. Persistent handles remain
/// serialized bytes: no cross-record owner relation proves that they are
/// position-independent. A shared finite work budget returns no identity when
/// canonicalization would exceed the decoder's bounded resource policy.
fn stable_object_record_identities(source_entry: &str, records: &[&[u8]]) -> Vec<Option<String>> {
    const MAX_GRAPH_WORK: usize = 8 * 1024 * 1024;

    let references = records
        .iter()
        .map(|bytes| {
            crate::om::counted_record_references(bytes, 0, records.len())
                .into_iter()
                .map(|reference| (reference.offset, reference.value as usize))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut graph_work = MAX_GRAPH_WORK;

    (0..records.len())
        .map(|root| {
            if references[root].is_empty() {
                return Some(stable_object_record_identity(source_entry, records[root]));
            }
            stable_object_record_graph_identity(
                source_entry,
                records,
                &references,
                root,
                &mut graph_work,
            )
        })
        .collect()
}

fn consume_stable_object_graph_work(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.checked_sub(amount)?;
    Some(())
}

fn append_stable_object_graph_node(
    seed: &mut Vec<u8>,
    graph_work: &mut usize,
    node_id: u64,
) -> Option<()> {
    consume_stable_object_graph_work(graph_work, 9)?;
    seed.push(STABLE_GRAPH_NODE_START);
    seed.extend_from_slice(&node_id.to_le_bytes());
    Some(())
}

const STABLE_GRAPH_NODE_START: u8 = 0xf0;

/// Encode one rooted object-record graph without depending on local ordinals.
fn stable_object_record_graph_identity(
    source_entry: &str,
    records: &[&[u8]],
    references: &[Vec<(usize, usize)>],
    root: usize,
    graph_work: &mut usize,
) -> Option<String> {
    #[derive(Debug)]
    struct Frame {
        record: usize,
        next_reference: usize,
        raw_cursor: usize,
    }

    const NODE_END: u8 = 0xf1;
    const RAW: u8 = 0xf2;
    const REFERENCE_NEW: u8 = 0xf3;
    const REFERENCE_BACK: u8 = 0xf4;

    let mut seed = Vec::new();
    consume_stable_object_graph_work(graph_work, source_entry.len().checked_add(32)?)?;
    seed.extend_from_slice(b"nx:om:object-record-graph\0");
    seed.extend_from_slice(&(source_entry.len() as u64).to_le_bytes());
    seed.extend_from_slice(source_entry.as_bytes());

    let mut node_ids = BTreeMap::<usize, u64>::new();
    let mut next_node_id = 0_u64;
    let mut stack = Vec::new();

    node_ids.insert(root, next_node_id);
    append_stable_object_graph_node(&mut seed, graph_work, next_node_id)?;
    next_node_id = next_node_id.checked_add(1)?;
    stack.push(Frame {
        record: root,
        next_reference: 0,
        raw_cursor: 0,
    });

    while let Some(frame_index) = stack.len().checked_sub(1) {
        let (record, next_reference, raw_cursor) = {
            let frame = stack.get(frame_index)?;
            (frame.record, frame.next_reference, frame.raw_cursor)
        };
        let record_bytes = *records.get(record)?;
        let record_references = references.get(record)?;
        if let Some(&(reference_offset, target)) = record_references.get(next_reference) {
            let reference_end = reference_offset.checked_add(3)?;
            if reference_offset < raw_cursor
                || reference_end > record_bytes.len()
                || target >= records.len()
            {
                return None;
            }
            let raw = record_bytes.get(raw_cursor..reference_offset)?;
            consume_stable_object_graph_work(graph_work, raw.len().checked_add(9)?)?;
            seed.push(RAW);
            seed.extend_from_slice(&(raw.len() as u64).to_le_bytes());
            seed.extend_from_slice(raw);

            let frame = stack.get_mut(frame_index)?;
            frame.next_reference = frame.next_reference.checked_add(1)?;
            frame.raw_cursor = reference_end;

            if let Some(&target_id) = node_ids.get(&target) {
                consume_stable_object_graph_work(graph_work, 9)?;
                seed.push(REFERENCE_BACK);
                seed.extend_from_slice(&target_id.to_le_bytes());
            } else {
                node_ids.insert(target, next_node_id);
                seed.push(REFERENCE_NEW);
                seed.extend_from_slice(&next_node_id.to_le_bytes());
                append_stable_object_graph_node(&mut seed, graph_work, next_node_id)?;
                next_node_id = next_node_id.checked_add(1)?;
                stack.push(Frame {
                    record: target,
                    next_reference: 0,
                    raw_cursor: 0,
                });
            }
        } else {
            if raw_cursor > record_bytes.len() {
                return None;
            }
            let raw = record_bytes.get(raw_cursor..)?;
            consume_stable_object_graph_work(graph_work, raw.len().checked_add(1)?)?;
            seed.push(RAW);
            seed.extend_from_slice(&(raw.len() as u64).to_le_bytes());
            seed.extend_from_slice(raw);
            seed.push(NODE_END);
            stack.pop();
        }
    }

    Some(format!(
        "nx:om:object-record:{}",
        cadmpeg_ir::hash::sha256_hex(&seed)
    ))
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
    /// Record-order-independent identity when the value is unique in the table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_identity: Option<String>,
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
    /// Content-backed identity when the scoped exact bytes are unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_identity: Option<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the block start.
    pub source_offset: u64,
}

/// Admitted complete grammar selected for one offset-store control block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBlockControlFormKind {
    ZeroPrefixed,
    ProductAnchored,
}

/// Atomic classification of one complete offset-store control lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlForm {
    /// Globally unique control-form identity.
    pub id: String,
    /// Opening control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Selected complete control grammar.
    pub kind: DataBlockControlFormKind,
    /// Number of values in the admitted control array.
    pub value_count: u32,
    /// Byte width of the compact leading value before a product-anchored array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_value_width: Option<u8>,
    /// Compact leading little-endian value before a product-anchored array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_value: Option<u32>,
    /// Exact serialized opening control-block length.
    pub byte_len: u64,
    /// Absolute file offset of the control block.
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

/// Ordered little-endian value preceding a store product anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlIndexValue {
    /// Globally unique value identity.
    pub id: String,
    /// Control block that opens the logical lane in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based value order in the aligned prefix array.
    pub ordinal: u32,
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
    /// Target in the native `class_definitions` arena when that registry slot
    /// has a retained declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_definition: Option<String>,
    /// Exact registered class name when that registry slot has a retained
    /// declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
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

/// Exact row encoding that selects `UGS::RM_creation_display_data` in an
/// `RMFastLoad` record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RmCreationDisplayDataEncoding {
    /// Self-framed index row whose fourth post-flag index selects the class.
    Index {
        flag: u8,
        indices: [u32; 4],
        raw_indices: [Vec<u8>; 4],
        index_source_offsets: [u64; 4],
    },
    /// Self-framed linked row whose third post-marker index selects the class.
    Linked {
        discriminator: u8,
        target_index: u32,
        raw_target_index: Vec<u8>,
        target_index_source_offset: u64,
        indices: [u32; 3],
        raw_indices: [Vec<u8>; 3],
        index_source_offsets: [u64; 3],
        flag: u8,
        mode: u8,
    },
    /// Self-framed target row whose third post-marker index selects the class.
    Target {
        target_index: u32,
        raw_target_index: Vec<u8>,
        target_index_source_offset: u64,
        indices: [u32; 3],
        raw_indices: [Vec<u8>; 3],
        index_source_offsets: [u64; 3],
        mode: u8,
    },
}

/// Lossless class-selected creation-display relation in `RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmCreationDisplayDataRelation {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based relation order in ascending source order.
    pub ordinal: u32,
    /// Leading compact index when the row encoding carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_index: Option<u32>,
    /// Exact serialized leading-index token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_first_index: Option<Vec<u8>>,
    /// Absolute file offset of the leading compact index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_index_source_offset: Option<u64>,
    /// Exact registered class name.
    pub class_name: String,
    /// Target in the native `class_definitions` arena.
    pub class_definition: String,
    /// Exact admitted row encoding.
    pub encoding: RmCreationDisplayDataEncoding,
    /// Member addressed by the target index when the row carries one and the
    /// index resolves in the `RMFastLoad` object-ID table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_object_id: Option<String>,
    /// Directory entry containing the relation.
    pub source_entry: String,
    /// Absolute file offset of the opening row discriminator.
    pub source_offset: u64,
}

/// Complete named NX part palette for color indices 1 through 216.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartColorTable {
    /// Globally unique table identity.
    pub id: String,
    /// Registered `UGS::COLOR_table` declaration in `class_definitions`.
    pub class_definition: String,
    /// Name of the separately encoded background color.
    pub background_name: String,
    /// Normalized background RGB components.
    pub background_rgb: [f32; 3],
    /// Exact serialized background component atoms.
    pub raw_background_components: [Vec<u8>; 3],
    /// Absolute file offsets of the background component atoms.
    pub background_component_source_offsets: [u64; 3],
    /// Ordered entries in the native `part_color_definitions` arena.
    pub definitions: Vec<String>,
    /// Directory entry containing the table.
    pub source_entry: String,
    /// Absolute file offset of the counted name roster.
    pub source_offset: u64,
}

/// One named RGB entry from an NX part palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartColorDefinition {
    /// Globally unique color-definition identity.
    pub id: String,
    /// Owning table in the native `part_color_tables` arena.
    pub color_table: String,
    /// One-based NX color index.
    pub color_index: u16,
    /// Serialized color name.
    pub name: String,
    /// Normalized RGB components.
    pub rgb: [f32; 3],
    /// Exact serialized index token.
    pub raw_color_index: Vec<u8>,
    /// Exact serialized component atoms.
    pub raw_components: [Vec<u8>; 3],
    /// Absolute file offset of the opening `05` marker.
    pub source_offset: u64,
    /// Absolute file offsets of the three component atoms.
    pub component_source_offsets: [u64; 3],
}

/// Exact row encoding carrying one `RMFastLoad` display-color assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RmDisplayColorAssignmentEncoding {
    /// Linked row with an unresolved leading object identity.
    Linked {
        /// Unresolved leading object identity.
        object_index: u32,
        /// Exact leading-object token.
        raw_object_index: Vec<u8>,
        /// Absolute leading-object token offset.
        object_index_source_offset: u64,
        /// Row discriminator.
        discriminator: u8,
        /// Target index.
        target_index: u32,
        /// Exact target-index token.
        raw_target_index: Vec<u8>,
        /// Absolute target-index token offset.
        target_index_source_offset: u64,
        /// Three post-marker indices.
        indices: [u32; 3],
        /// Exact post-marker index tokens.
        raw_indices: [Vec<u8>; 3],
        /// Absolute post-marker token offsets.
        index_source_offsets: [u64; 3],
        /// Row flag.
        flag: u8,
        /// Row mode.
        mode: u8,
    },
    /// Target-index row without a leading object identity.
    Target {
        /// Target index.
        target_index: u32,
        /// Exact target-index token.
        raw_target_index: Vec<u8>,
        /// Absolute target-index token offset.
        target_index_source_offset: u64,
        /// Three post-marker indices.
        indices: [u32; 3],
        /// Exact post-marker index tokens.
        raw_indices: [Vec<u8>; 3],
        /// Absolute post-marker token offsets.
        index_source_offsets: [u64; 3],
        /// Row mode.
        mode: u8,
    },
}

/// Explicit color assignment carried by one complete `RMFastLoad` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmDisplayColorAssignment {
    /// Globally unique assignment identity.
    pub id: String,
    /// Zero-based source order.
    pub ordinal: u32,
    /// Complete self-framed row carrying the color token.
    pub encoding: RmDisplayColorAssignmentEncoding,
    /// Member addressed by the row target index when it resolves in the
    /// `RMFastLoad` object-ID table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_object_id: Option<String>,
    /// One-based part palette index.
    pub color_index: u16,
    /// Target in `part_color_definitions`.
    pub color_definition: String,
    /// Exact color-index token.
    pub raw_color_index: Vec<u8>,
    /// Owning directory entry.
    pub source_entry: String,
    /// Absolute color-token offset.
    pub source_offset: u64,
    /// Absolute row-opener offset.
    pub row_source_offset: u64,
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

/// Return a content-backed identity for one offset-store block.
///
/// The source entry and block role scope the exact bytes. Callers must only
/// admit the value when this key is unique in that scope; equal bytes at two
/// positions are not distinguishable without an additional serialized owner.
pub(crate) fn stable_data_block_identity(
    source_entry: &str,
    role: DataBlockRole,
    bytes: &[u8],
) -> String {
    let mut seed = Vec::with_capacity(source_entry.len() + bytes.len() + 18);
    seed.extend_from_slice(b"nx:om:data-block\0");
    seed.extend_from_slice(source_entry.as_bytes());
    seed.push(0);
    seed.push(match role {
        DataBlockRole::Control => 0,
        DataBlockRole::Column => 1,
    });
    seed.extend_from_slice(bytes);
    format!("nx:om:data-block:{}", cadmpeg_ir::hash::sha256_hex(&seed))
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

/// Canonical UUID text spanning one or more contiguous bounded OM records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectUuidValue {
    /// Globally unique value identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Exact UUID text.
    pub uuid: String,
    /// Bounded OM records intersected by the complete UUID frame.
    pub records: Vec<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `03 26` marker.
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

/// Exact two-token persistent-handle run in one bounded OM object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecordHandlePair {
    /// Globally unique pair identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
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
    /// Non-decreasing persistent handles; only the serialized closing duplicate is omitted.
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
                [b'I', b'I', 42, 0, ..] => ("little_endian", 42, View::u32_le_at(payload, 4)?),
                [b'M', b'M', 0, 42, ..] => ("big_endian", 42, View::u32_be_at(payload, 4)?),
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
            let bytes = container
                .bounded_entry_bytes(source_offset, u64::try_from(record.byte_len).ok()?)?;
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
            let bytes = container.bounded_entry_bytes(record.source_offset, record.byte_len)?;
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
            let Some(source_offset) = record.source_offset.checked_add(record.prefix_byte_len)
            else {
                return Vec::new();
            };
            let Some(bytes) = container.bounded_entry_bytes(source_offset, record.tail_byte_len)
            else {
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
                        source_offset: source_offset + offset as u64,
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
        for (ordinal, definition) in section.types.iter().cloned().enumerate() {
            let registry_fields = class_registry_fields(&definition);
            definitions.insert(
                (entry_index, definition.offset),
                ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_storage_code: registry_fields.storage_code,
                    registry_base_class: registry_fields.base_class,
                    registry_reference: registry_fields.reference,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix: registry_fields.layout_prefix,
                    schema_fingerprint: registry_fields.schema_fingerprint,
                    layout_terminal: registry_fields.layout_terminal,
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
        for (ordinal, definition) in section.types.iter().cloned().enumerate() {
            let registry_fields = class_registry_fields(&definition);
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_storage_code: registry_fields.storage_code,
                    registry_base_class: registry_fields.base_class,
                    registry_reference: registry_fields.reference,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix: registry_fields.layout_prefix,
                    schema_fingerprint: registry_fields.schema_fingerprint,
                    layout_terminal: registry_fields.layout_terminal,
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

fn registry_layout_fields(suffix: &[u8]) -> (Vec<u8>, Option<[u8; 8]>, Option<u8>) {
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

struct ClassRegistryFields {
    layout_prefix: Vec<u8>,
    schema_fingerprint: Option<[u8; 8]>,
    layout_terminal: Option<u8>,
    storage_code: Option<u32>,
    base_class: Option<u32>,
    reference: Option<u32>,
}

fn class_registry_fields(definition: &OmTypeDefinition<'_>) -> ClassRegistryFields {
    let (layout_prefix, legacy_fingerprint, layout_terminal) =
        registry_layout_fields(definition.registry_suffix);
    let registry = definition.class_registry_layout();
    ClassRegistryFields {
        layout_prefix,
        schema_fingerprint: registry
            .map(|layout| layout.schema_fingerprint)
            .or(legacy_fingerprint),
        layout_terminal,
        storage_code: registry.map(|layout| layout.storage_code.value),
        base_class: registry.map(|layout| layout.base_class),
        reference: registry.map(|layout| layout.reference),
    }
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
        for (ordinal, definition) in section.fields.iter().cloned().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                registry_layout_fields(definition.registry_suffix);
            let registry = definition.field_registry_layout();
            definitions.insert(
                (entry_index, definition.offset),
                FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_storage_code: registry.map(|layout| layout.storage_code.value),
                    registry_owner_class: registry.map(|layout| layout.owner_class),
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
        for (ordinal, definition) in section.fields.iter().cloned().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                registry_layout_fields(definition.registry_suffix);
            let registry = definition.field_registry_layout();
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_storage_code: registry.map(|layout| layout.storage_code.value),
                    registry_owner_class: registry.map(|layout| layout.owner_class),
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

/// Catalog every externally bounded NX OM entity record.
pub fn object_records(container: &Container) -> Vec<ObjectRecord> {
    let mut candidates = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        let record_bytes = records
            .iter()
            .map(|record| record.bytes)
            .collect::<Vec<_>>();
        let stable_identities = stable_object_record_identities(&entry.name, &record_bytes);
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
        for (record_ordinal, record) in records.iter().cloned().enumerate() {
            let record_id =
                |ordinal| format!("nx:om-record-directory-{section_ordinal}:entry#{ordinal}");
            candidates.push((
                section_ordinal,
                record_ordinal,
                section_offset,
                entry_offset,
                entry.name.clone(),
                record,
                stable_identities[record_ordinal].clone(),
                dependencies
                    .get(&record_ordinal)
                    .into_iter()
                    .flatten()
                    .map(|ordinal| record_id(*ordinal))
                    .collect::<Vec<_>>(),
                dependents
                    .get(&record_ordinal)
                    .into_iter()
                    .flatten()
                    .map(|ordinal| record_id(*ordinal))
                    .collect::<Vec<_>>(),
            ));
        }
    }

    let mut identity_counts = BTreeMap::<String, usize>::new();
    for (_, _, _, _, source_entry, _, stable_identity, _, _) in &candidates {
        let Some(stable_identity) = stable_identity else {
            continue;
        };
        let identity = format!("{source_entry}\0{stable_identity}");
        *identity_counts.entry(identity).or_default() += 1;
    }

    candidates
        .into_iter()
        .map(
            |(
                section_ordinal,
                record_ordinal,
                section_offset,
                entry_offset,
                source_entry,
                record,
                stable_identity,
                dependencies,
                dependents,
            )| {
                let stable_identity = stable_identity.filter(|identity| {
                    let key = format!("{source_entry}\0{identity}");
                    identity_counts.get(&key) == Some(&1)
                });
                ObjectRecord {
                    id: format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                    object_id: Some(record.object_id.0),
                    object_id_source_offset: Some(entry_offset + record.object_id.1),
                    section_ordinal: section_ordinal as u32,
                    record_ordinal: record_ordinal as u32,
                    section_offset,
                    byte_len: record.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                    stable_identity,
                    dependencies,
                    dependents,
                    source_entry,
                    source_offset: entry_offset + record.offset as u64,
                }
            },
        )
        .collect()
}

/// Retain the complete counted `RMFastLoad` active-object membership table.
pub fn rmfastload_object_id_table(
    container: &Container,
) -> Option<(RmFastLoadObjectIdTable, Vec<RmFastLoadObjectId>)> {
    let (entry, table) = container.rmfastload_object_id_table()?;
    let entry_offset = entry.file_span?.0;
    let table_id = "nx:rmfastload:object-id-table#0".to_string();
    let mut object_ids = table
        .object_ids
        .into_iter()
        .enumerate()
        .map(|(ordinal, object_id)| RmFastLoadObjectId {
            id: format!("nx:rmfastload:object-id#{ordinal:010}"),
            table: table_id.clone(),
            ordinal: ordinal as u32,
            value: object_id.value,
            stable_identity: None,
            raw: object_id.raw,
            source_offset: entry_offset + object_id.offset as u64,
        })
        .collect::<Vec<_>>();
    assign_rmfastload_object_id_identities(&mut object_ids);
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

/// Assign value-backed witnesses only when an active membership value is
/// unique in its owning table. The ordinal identity remains authoritative for
/// table-indexed references such as display targets.
fn assign_rmfastload_object_id_identities(entries: &mut [RmFastLoadObjectId]) {
    let mut counts = BTreeMap::<u32, usize>::new();
    for entry in entries.iter() {
        *counts.entry(entry.value).or_default() += 1;
    }
    for entry in entries.iter_mut() {
        entry.stable_identity = (counts.get(&entry.value) == Some(&1))
            .then(|| format!("{}:value#{}", entry.table, entry.value));
    }
}

/// Catalog every externally bounded block in offset-only NX OM storage.
pub fn data_blocks(container: &Container) -> Vec<DataBlock> {
    let mut candidates = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        candidates.push((
            section_ordinal,
            0usize,
            DataBlockRole::Control,
            entry.name.clone(),
            section_offset,
            entry_offset,
            control.clone(),
        ));
        candidates.extend(
            records
                .iter()
                .cloned()
                .enumerate()
                .map(|(record_ordinal, block)| {
                    (
                        section_ordinal,
                        record_ordinal + 1,
                        DataBlockRole::Column,
                        entry.name.clone(),
                        section_offset,
                        entry_offset,
                        block,
                    )
                }),
        );
    }

    let mut identity_counts = BTreeMap::<String, usize>::new();
    for (_, _, role, source_entry, _, _, block) in &candidates {
        let identity = stable_data_block_identity(source_entry, *role, block.bytes);
        *identity_counts.entry(identity).or_default() += 1;
    }

    candidates
        .into_iter()
        .map(
            |(
                section_ordinal,
                block_ordinal,
                role,
                source_entry,
                section_offset,
                entry_offset,
                block,
            )| {
                let sha256 = cadmpeg_ir::hash::sha256_hex(block.bytes);
                let stable_identity = stable_data_block_identity(&source_entry, role, block.bytes);
                DataBlock {
                    id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    block_ordinal: block_ordinal as u32,
                    role,
                    section_offset,
                    byte_len: block.bytes.len() as u64,
                    sha256,
                    stable_identity: (identity_counts.get(&stable_identity) == Some(&1))
                        .then_some(stable_identity),
                    source_entry,
                    source_offset: entry_offset + block.offset as u64,
                }
            },
        )
        .collect()
}

/// Classify every admitted complete offset-only store control lane.
pub fn data_block_control_forms(container: &Container) -> Vec<DataBlockControlForm> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .filter_map(|(section_ordinal, (entry, section))| {
            let (control, _, records) = section.as_offset_only()?;
            let (kind, leading_value_width, leading_value, value_count) =
                match crate::om::offset_store_control_form(
                    control.bytes,
                    records.first().map(|record| record.bytes),
                )? {
                    crate::om::OffsetStoreControlForm::ZeroPrefixed { values } => (
                        DataBlockControlFormKind::ZeroPrefixed,
                        None,
                        None,
                        values.len(),
                    ),
                    crate::om::OffsetStoreControlForm::ProductAnchored {
                        leading_value,
                        values,
                    } => {
                        let (leading_value_width, leading_value) = match leading_value {
                            Some((width, value)) => (Some(u8::try_from(width).ok()?), Some(value)),
                            None => (None, None),
                        };
                        (
                            DataBlockControlFormKind::ProductAnchored,
                            leading_value_width,
                            leading_value,
                            values.len(),
                        )
                    }
                };
            Some(DataBlockControlForm {
                id: format!("nx:om-data-block-control-forms:form#{section_ordinal}"),
                data_block: format!("nx:om-data-blocks-{section_ordinal}:block#0"),
                kind,
                value_count: u32::try_from(value_count).ok()?,
                leading_value_width,
                leading_value,
                byte_len: control.bytes.len() as u64,
                source_offset: entry.file_span.map_or(0, |(offset, _)| offset)
                    + control.offset as u64,
            })
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
            let Some((control, _, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(crate::om::OffsetStoreControlForm::ZeroPrefixed { values }) =
                crate::om::offset_store_control_form(
                    control.bytes,
                    records.first().map(|record| record.bytes),
                )
            else {
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
            let Some((control, _, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            if !matches!(
                crate::om::offset_store_control_form(
                    control.bytes,
                    records.first().map(|record| record.bytes),
                ),
                Some(crate::om::OffsetStoreControlForm::ZeroPrefixed { .. })
            ) {
                return Vec::new();
            }
            let mut registry = BTreeMap::new();
            for definition in container
                .om_sections()
                .into_iter()
                .filter(|(candidate, _)| std::ptr::eq(*candidate, entry))
                .flat_map(|(_, section)| section.types.iter().cloned().collect::<Vec<_>>())
                .chain(
                    container
                        .indexed_om_sections()
                        .into_iter()
                        .filter(|(candidate, _)| std::ptr::eq(*candidate, entry))
                        .flat_map(|(_, section)| {
                            std::sync::Arc::as_ref(&section.types).to_owned()
                        }),
                )
            {
                registry.entry(definition.offset).or_insert(definition);
            }
            let registry = registry.into_values().collect::<Vec<_>>();
            let Some(ordinals) = crate::om::offset_store_control_class_ordinals(control.bytes)
            else {
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
                .map(|(ordinal, class_ordinal)| {
                    let definition = usize::try_from(class_ordinal)
                        .ok()
                        .and_then(|ordinal| registry.get(ordinal));
                    DataBlockControlClassReference {
                        id: format!(
                            "nx:om-data-block-control-class-references-{section_ordinal}:class#{ordinal}"
                        ),
                        data_block: data_block.clone(),
                        ordinal: ordinal as u32,
                        class_ordinal,
                        class_definition: definition.map(|definition| {
                            format!("nx:om-entry-{entry_index}:class#{}", definition.offset)
                        }),
                        class_name: definition.map(|definition| definition.name.to_string()),
                        source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                    }
                })
                .collect()
        })
        .collect()
}

/// Decode aligned index arrays preceding a unique control-lane product anchor.
pub fn data_block_control_index_values(container: &Container) -> Vec<DataBlockControlIndexValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some((control, _, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(crate::om::OffsetStoreControlForm::ProductAnchored {
                leading_value,
                values,
            }) = crate::om::offset_store_control_form(
                control.bytes,
                records.first().map(|record| record.bytes),
            )
            else {
                return Vec::new();
            };
            let leading_value_width = leading_value.map_or(0, |(width, _)| width);
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            let block_count = records.len() + 1;
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlIndexValue {
                    id: format!(
                        "nx:om-data-block-control-index-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    value,
                    target_data_block: control_index_data_block(
                        section_ordinal,
                        block_count,
                        value,
                    ),
                    source_offset: entry_offset
                        + control.offset as u64
                        + leading_value_width as u64
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
            let Some((control, _, _)) = section.as_offset_only() else {
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
pub fn data_block_references(
    container: &Container,
    object_records: &[ObjectRecord],
    expression_declarations: &[ExpressionDeclaration],
) -> Vec<DataBlockReference> {
    let mut target_records = BTreeMap::<(String, u32), Vec<String>>::new();
    for record in object_records {
        let Some(object_id) = record.object_id else {
            continue;
        };
        target_records
            .entry((record.source_entry.clone(), object_id))
            .or_default()
            .push(record.id.clone());
    }
    let mut declarations = BTreeMap::<(String, u32), Vec<String>>::new();
    for declaration in expression_declarations {
        declarations
            .entry((declaration.source_entry.clone(), declaration.object_id))
            .or_default()
            .push(declaration.id.clone());
    }
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some((control, _, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let mut source_blocks = Vec::with_capacity(records.len() + 1);
            source_blocks.push(control.clone());
            source_blocks.extend(records.iter().cloned());
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
                                target_record: unique(target_records.get(&key)),
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
            let Some((_, _, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let block_count = records.len() + 1;
            records
                .iter()
                .cloned()
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
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let source_base = entry_offset + storage_offset as u64;
            let block_count = records.len() + 1;
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
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = records.len() + 1;
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
                        records,
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
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = records.len() + 1;
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
                        records,
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
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = records.len() + 1;
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
                        records,
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

/// Decode class-selected creation-display relations from `RMFastLoad` record
/// areas. The compact indices remain uninterpreted until their object roles are
/// established independently.
pub fn rm_creation_display_data_relations(
    container: &Container,
    object_ids: &[RmFastLoadObjectId],
) -> Vec<RmCreationDisplayDataRelation> {
    const CLASS_NAME: &str = "UGS::RM_creation_display_data";

    let mut relations = Vec::new();
    for (entry, section) in container
        .om_sections()
        .into_iter()
        .filter(|(entry, _)| entry.name == "/Root/FastLoad/RMFastLoad")
    {
        let Some(record_area) = section.record_area else {
            continue;
        };
        let Some(record_area_offset) = section.record_area_offset else {
            continue;
        };
        let Some((class_ordinal, definition)) = section
            .types
            .iter()
            .enumerate()
            .find(|(_, definition)| definition.name == CLASS_NAME)
        else {
            continue;
        };
        let Ok(class_ordinal) = u32::try_from(class_ordinal) else {
            continue;
        };
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let source_base = entry_offset + record_area_offset as u64;
        let class_definition = format!("nx:om-entry-{entry_index}:class#{}", definition.offset);

        for row in crate::om::offset_store_index_rows(record_area) {
            if row.indices[3].0 != class_ordinal {
                continue;
            }
            relations.push(RmCreationDisplayDataRelation {
                id: String::new(),
                ordinal: 0,
                first_index: Some(row.first_index),
                raw_first_index: Some(row.raw_first_index),
                first_index_source_offset: Some(source_base + row.first_index_offset as u64),
                class_name: CLASS_NAME.to_string(),
                class_definition: class_definition.clone(),
                encoding: RmCreationDisplayDataEncoding::Index {
                    flag: row.flag,
                    indices: row.indices.map(|(index, _)| index),
                    raw_indices: row.raw_indices,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                },
                target_object_id: None,
                source_entry: entry.name.clone(),
                source_offset: source_base + row.offset as u64,
            });
        }
        for row in crate::om::offset_store_linked_index_rows(record_area) {
            if row.indices[2].0 != class_ordinal {
                continue;
            }
            relations.push(RmCreationDisplayDataRelation {
                id: String::new(),
                ordinal: 0,
                first_index: Some(row.first_index.0),
                raw_first_index: Some(row.raw_first_index),
                first_index_source_offset: Some(source_base + row.first_index.1 as u64),
                class_name: CLASS_NAME.to_string(),
                class_definition: class_definition.clone(),
                encoding: RmCreationDisplayDataEncoding::Linked {
                    discriminator: row.discriminator,
                    target_index: row.target_index.0,
                    raw_target_index: row.raw_target_index,
                    target_index_source_offset: source_base + row.target_index.1 as u64,
                    indices: row.indices.map(|(index, _)| index),
                    raw_indices: row.raw_indices,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                    flag: row.flag,
                    mode: row.mode,
                },
                target_object_id: rmfastload_target_object_id(object_ids, row.target_index.0),
                source_entry: entry.name.clone(),
                source_offset: source_base + row.offset as u64,
            });
        }
        for row in crate::om::offset_store_target_index_rows(record_area) {
            if row.indices[2].0 != class_ordinal {
                continue;
            }
            relations.push(RmCreationDisplayDataRelation {
                id: String::new(),
                ordinal: 0,
                first_index: None,
                raw_first_index: None,
                first_index_source_offset: None,
                class_name: CLASS_NAME.to_string(),
                class_definition: class_definition.clone(),
                encoding: RmCreationDisplayDataEncoding::Target {
                    target_index: row.target_index.0,
                    raw_target_index: row.raw_target_index,
                    target_index_source_offset: source_base + row.target_index.1 as u64,
                    indices: row.indices.map(|(index, _)| index),
                    raw_indices: row.raw_indices,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                    mode: row.mode,
                },
                target_object_id: rmfastload_target_object_id(object_ids, row.target_index.0),
                source_entry: entry.name.clone(),
                source_offset: source_base + row.offset as u64,
            });
        }
    }

    relations.sort_by_key(|relation| relation.source_offset);
    for (ordinal, relation) in relations.iter_mut().enumerate() {
        relation.ordinal = ordinal as u32;
        relation.id = format!("nx:rm-creation-display-data-relations:relation#{ordinal}");
    }
    relations
}

/// Decode complete part-local color tables from class-declaring offset stores.
pub fn part_color_tables(container: &Container) -> (Vec<PartColorTable>, Vec<PartColorDefinition>) {
    const CLASS_NAME: &str = "UGS::COLOR_table";
    let mut tables = Vec::new();
    let mut definitions = Vec::new();

    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        let Some((_, storage, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(storage_offset) = records.first().map(|record| record.offset) else {
            continue;
        };
        let Some(class) = section
            .types
            .iter()
            .find(|definition| definition.name == CLASS_NAME)
        else {
            continue;
        };
        let parsed_tables = crate::om::color_tables(storage);
        let [table] = parsed_tables.as_slice() else {
            continue;
        };
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let source_base = entry_offset + storage_offset as u64;
        let table_id = format!("nx:part-color-tables:table#{section_ordinal}");
        let definition_ids = table
            .definitions
            .iter()
            .map(|definition| {
                format!(
                    "nx:part-color-definitions-{section_ordinal}:color#{}",
                    definition.color_index
                )
            })
            .collect::<Vec<_>>();
        definitions.extend(table.definitions.iter().zip(&definition_ids).map(
            |(definition, id)| {
                PartColorDefinition {
                    id: id.clone(),
                    color_table: table_id.clone(),
                    color_index: definition.color_index,
                    name: definition.name.to_string(),
                    rgb: definition.rgb,
                    raw_color_index: definition.raw_color_index.clone(),
                    raw_components: definition.raw_components.clone(),
                    source_offset: source_base + definition.offset as u64,
                    component_source_offsets: definition
                        .component_offsets
                        .map(|offset| source_base + offset as u64),
                }
            },
        ));
        tables.push(PartColorTable {
            id: table_id,
            class_definition: format!("nx:om-entry-{entry_index}:class#{}", class.offset),
            background_name: table.background_name.to_string(),
            background_rgb: table.background_rgb,
            raw_background_components: table.raw_background_components.clone(),
            background_component_source_offsets: table
                .background_component_offsets
                .map(|offset| source_base + offset as u64),
            definitions: definition_ids,
            source_entry: entry.name.clone(),
            source_offset: source_base + table.offset as u64,
        });
    }

    (tables, definitions)
}

/// Decode explicit display-color assignments from `RMFastLoad` linked rows.
pub fn rm_display_color_assignments(
    container: &Container,
    color_definitions: &[PartColorDefinition],
    object_ids: &[RmFastLoadObjectId],
) -> Vec<RmDisplayColorAssignment> {
    let mut assignments = Vec::new();
    for (entry, section) in container
        .om_sections()
        .into_iter()
        .filter(|(entry, _)| entry.name == "/Root/FastLoad/RMFastLoad")
    {
        let (Some(record_area), Some(record_area_offset)) =
            (section.record_area, section.record_area_offset)
        else {
            continue;
        };
        let source_base =
            entry.file_span.map_or(0, |(offset, _)| offset) + record_area_offset as u64;
        for row in crate::om::offset_store_linked_index_rows(record_area) {
            let Some(color) = crate::om::linked_row_color_index(record_area, &row) else {
                continue;
            };
            let mut matches = color_definitions
                .iter()
                .filter(|definition| definition.color_index == color.color_index);
            let Some(definition) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            assignments.push(RmDisplayColorAssignment {
                id: String::new(),
                ordinal: 0,
                encoding: RmDisplayColorAssignmentEncoding::Linked {
                    object_index: row.first_index.0,
                    raw_object_index: row.raw_first_index,
                    object_index_source_offset: source_base + row.first_index.1 as u64,
                    discriminator: row.discriminator,
                    target_index: row.target_index.0,
                    raw_target_index: row.raw_target_index,
                    target_index_source_offset: source_base + row.target_index.1 as u64,
                    indices: row.indices.map(|(index, _)| index),
                    raw_indices: row.raw_indices,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                    flag: row.flag,
                    mode: row.mode,
                },
                target_object_id: rmfastload_target_object_id(object_ids, row.target_index.0),
                color_index: color.color_index,
                color_definition: definition.id.clone(),
                raw_color_index: color.raw_color_index,
                source_entry: entry.name.clone(),
                source_offset: source_base + color.offset as u64,
                row_source_offset: source_base + row.offset as u64,
            });
        }
        for row in crate::om::offset_store_target_index_rows(record_area) {
            let Some(color) = crate::om::target_row_color_index(record_area, &row) else {
                continue;
            };
            let mut matches = color_definitions
                .iter()
                .filter(|definition| definition.color_index == color.color_index);
            let Some(definition) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            assignments.push(RmDisplayColorAssignment {
                id: String::new(),
                ordinal: 0,
                encoding: RmDisplayColorAssignmentEncoding::Target {
                    target_index: row.target_index.0,
                    raw_target_index: row.raw_target_index,
                    target_index_source_offset: source_base + row.target_index.1 as u64,
                    indices: row.indices.map(|(index, _)| index),
                    raw_indices: row.raw_indices,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                    mode: row.mode,
                },
                target_object_id: rmfastload_target_object_id(object_ids, row.target_index.0),
                color_index: color.color_index,
                color_definition: definition.id.clone(),
                raw_color_index: color.raw_color_index,
                source_entry: entry.name.clone(),
                source_offset: source_base + color.offset as u64,
                row_source_offset: source_base + row.offset as u64,
            });
        }
    }
    assignments.sort_by_key(|assignment| assignment.source_offset);
    for (ordinal, assignment) in assignments.iter_mut().enumerate() {
        assignment.ordinal = ordinal as u32;
        assignment.id = format!("nx:rm-display-color-assignments:assignment#{ordinal}");
    }
    assignments
}

fn rmfastload_target_object_id(object_ids: &[RmFastLoadObjectId], target: u32) -> Option<String> {
    let target = usize::try_from(target).ok()?;
    object_ids.get(target).map(|object_id| object_id.id.clone())
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
            match &section.store {
                IndexedStore::Fixed { records } => records.iter().find_map(|record| {
                    crate::om::store_version(record.bytes, record.offset).map(|version| {
                        StoreHeader {
                            id: format!("nx:om-store-headers:store#{section_ordinal}"),
                            section_ordinal: section_ordinal as u32,
                            object_id: Some(record.object_id.0),
                            version: version.value.to_string(),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + version.offset as u64,
                        }
                    })
                }),
                IndexedStore::OffsetOnly {
                    control, records, ..
                } => std::iter::once(control)
                    .chain(records.iter())
                    .find_map(|record| {
                        crate::om::store_version(record.bytes, record.offset).map(|version| {
                            StoreHeader {
                                id: format!("nx:om-store-headers:store#{section_ordinal}"),
                                section_ordinal: section_ordinal as u32,
                                object_id: None,
                                version: version.value.to_string(),
                                source_entry: entry.name.clone(),
                                source_offset: entry_offset + version.offset as u64,
                            }
                        })
                    }),
            }
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
            if section.as_fixed().is_none() {
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

/// Decode canonical UUID frames across the contiguous storage of ID-bounded
/// OM records. A value retains every physical record intersected by its frame.
pub fn object_uuid_values(container: &Container) -> Vec<ObjectUuidValue> {
    const FRAME_LEN: usize = 2 + 36 + 1;
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(records) = section.as_fixed() else {
                return Vec::new();
            };
            let Some(first) = records.first() else {
                return Vec::new();
            };
            let Some(last) = records.last() else {
                return Vec::new();
            };
            if records.windows(2).any(|window| {
                window[0].offset.checked_add(window[0].bytes.len()) != Some(window[1].offset)
            }) {
                return Vec::new();
            }
            let Some(end) = last.offset.checked_add(last.bytes.len()) else {
                return Vec::new();
            };
            let Some((entry_offset, _)) = entry.file_span else {
                return Vec::new();
            };
            let Ok(entry_offset_usize) = usize::try_from(entry_offset) else {
                return Vec::new();
            };
            let Some(storage_start) = entry_offset_usize.checked_add(first.offset) else {
                return Vec::new();
            };
            let Some(storage_end) = entry_offset_usize.checked_add(end) else {
                return Vec::new();
            };
            let Some(storage) = container.data.get(storage_start..storage_end) else {
                return Vec::new();
            };
            crate::om::uuid_string_values(storage, first.offset)
                .into_iter()
                .filter_map(|value| {
                    let frame_end = value.offset.checked_add(FRAME_LEN)?;
                    let records = records
                        .iter()
                        .enumerate()
                        .filter(|(_, record)| {
                            record.offset < frame_end
                                && record
                                    .offset
                                    .checked_add(record.bytes.len())
                                    .is_some_and(|record_end| value.offset < record_end)
                        })
                        .map(|(record_ordinal, _)| {
                            format!(
                                "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                            )
                        })
                        .collect::<Vec<_>>();
                    (!records.is_empty()).then(|| ObjectUuidValue {
                        id: format!(
                            "nx:om-object-uuid-values-{section_ordinal}:value#{}",
                            value.offset
                        ),
                        section_ordinal: section_ordinal as u32,
                        uuid: value.value.to_owned(),
                        records,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + value.offset as u64,
                    })
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
            if section.as_fixed().is_none() {
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

/// Join maximal two-token adjacent persistent-handle runs within object records.
pub fn object_record_handle_pairs(references: &[ObjectReference]) -> Vec<ObjectRecordHandlePair> {
    let mut by_record = BTreeMap::<&str, Vec<&ObjectReference>>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        by_record
            .entry(reference.record.as_str())
            .or_default()
            .push(reference);
    }
    let mut pairs = Vec::new();
    for (record, mut record_references) in by_record {
        record_references.sort_by_key(|reference| reference.source_offset);
        let mut at = 0;
        while at < record_references.len() {
            let start = at;
            while record_references
                .get(at + 1)
                .is_some_and(|next| next.source_offset == record_references[at].source_offset + 5)
            {
                at += 1;
            }
            let run = &record_references[start..=at];
            if let [first, second] = run {
                pairs.push(ObjectRecordHandlePair {
                    id: format!("nx:om-object-record:handle-pair#{}", first.source_offset),
                    record: record.to_string(),
                    object_id: first.object_id,
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
            let Some(records) = section.as_fixed() else {
                return Vec::new();
            };
            records
                .iter()
                .cloned()
                .enumerate()
                .filter_map(|(record_ordinal, record)| {
                    let object_id = record.object_id.0;
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
                    crate::om::ExpressionUnit::Inch => ExpressionUnit::Inch,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                    crate::om::ExpressionUnit::Native(unit) => ExpressionUnit::Native(unit),
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
    let mut name_counts = BTreeMap::<(String, String, ExpressionUnit), usize>::new();
    for expression in expressions.iter() {
        *name_counts
            .entry((
                expression_scope(expression).to_string(),
                expression.name.clone(),
                expression.unit.clone(),
            ))
            .or_default() += 1;
    }
    let mut values = BTreeMap::<(String, String, ExpressionUnit), f64>::new();
    for expression in expressions.iter_mut() {
        let key = (
            expression_scope(expression).to_string(),
            expression.name.clone(),
            expression.unit.clone(),
        );
        if name_counts.get(&key) != Some(&1) {
            expression.value = None;
            continue;
        }
        if let Some(value) = expression.value {
            values.insert(key, value);
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
                expression.unit.clone(),
            );
            if name_counts.get(&expression_key) != Some(&1) {
                continue;
            }
            let evaluated = evaluate_parameterized_expression(&expression.expression, |name| {
                let key = (
                    expression_scope(expression).to_string(),
                    name.to_string(),
                    expression.unit.clone(),
                );
                if name_counts.get(&key) != Some(&1) {
                    return None;
                }
                values.get(&key).copied()
            });
            if let Some(value) = evaluated {
                expression.value = Some(value);
                values.insert(expression_key.clone(), value);
                changed = true;
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
    mod native_units;
    mod state_counters;
    use std::io::{Cursor, Write};

    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use super::*;
    use crate::container;

    use crate::test_support::*;
    use crate::NxCodec;

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
    fn nx_expression_graph_scopes_equal_names_by_declared_unit() {
        let expression =
            |id: &str, name: &str, unit: super::ExpressionUnit, formula: &str, value| {
                super::Expression {
                    id: id.into(),
                    object_id: None,
                    record: None,
                    declaration: None,
                    name: name.into(),
                    parameter_index: None,
                    qualifier: None,
                    unit,
                    expression: formula.into(),
                    value,
                    source_entry: "part".into(),
                    source_table: "table".into(),
                    source_offset: 0,
                }
            };
        let mut expressions = vec![
            expression(
                "length-p1",
                "p1",
                super::ExpressionUnit::Millimeter,
                "5",
                Some(5.0),
            ),
            expression(
                "angle-p1",
                "p1",
                super::ExpressionUnit::Degree,
                "45",
                Some(45.0),
            ),
            expression(
                "length-p2",
                "p2",
                super::ExpressionUnit::Millimeter,
                "p1 * 2",
                None,
            ),
            expression(
                "angle-p2",
                "p2",
                super::ExpressionUnit::Degree,
                "p1 / 3",
                None,
            ),
        ];

        super::evaluate_expression_graphs(&mut expressions);

        assert_eq!(expressions[0].value, Some(5.0));
        assert_eq!(expressions[1].value, Some(45.0));
        assert_eq!(expressions[2].value, Some(10.0));
        assert_eq!(expressions[3].value, Some(15.0));
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
    fn nx_formula_dependencies_bind_equal_names_within_declared_unit() {
        let expression = |key: u32, name: &str, unit: super::ExpressionUnit, text: &str, value| {
            super::Expression {
                id: format!("nx:test:expression#{key}"),
                object_id: Some(key),
                record: None,
                declaration: None,
                name: name.into(),
                parameter_index: Some(key),
                qualifier: None,
                unit,
                expression: text.into(),
                value,
                source_entry: "/Root/UG_PART/UG_PART".into(),
                source_table: "table".into(),
                source_offset: u64::from(key),
            }
        };
        let expressions = [
            expression(10, "p1", super::ExpressionUnit::Millimeter, "5", Some(5.0)),
            expression(11, "p1", super::ExpressionUnit::Degree, "45", Some(45.0)),
            expression(
                20,
                "p2",
                super::ExpressionUnit::Millimeter,
                "p1 * 2",
                Some(10.0),
            ),
            expression(
                21,
                "p2",
                super::ExpressionUnit::Degree,
                "p1 / 3",
                Some(15.0),
            ),
        ];
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert_eq!(
            ir.model.parameters[2].dependencies,
            [ir.model.parameters[0].id.clone()]
        );
        assert_eq!(
            ir.model.parameters[3].dependencies,
            [ir.model.parameters[1].id.clone()]
        );
        assert_eq!(
            ir.model.parameters[0]
                .properties
                .get("unit")
                .map(String::as_str),
            Some("millimeter")
        );
        assert_eq!(
            ir.model.parameters[1]
                .properties
                .get("unit")
                .map(String::as_str),
            Some("degree")
        );
        assert!(crate::decode::incomplete_expression_parameters(&ir).is_empty());

        ir.model.parameters[0]
            .properties
            .insert("unit".into(), "native".into());
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&ir),
            [
                ir.model.parameters[0].id.clone(),
                ir.model.parameters[2].id.clone(),
            ]
            .into()
        );
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
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

        crate::native::attach::attach_expression_parameters(
            &mut ir,
            &expressions,
            &[],
            &[],
            &mut annotations,
        );

        assert_eq!(ir.model.features.len(), 2);
        assert_eq!(
            ir.model.features[0].id.as_str(),
            "table-b:feature#equations"
        );
        assert_eq!(ir.model.features[0].ordinal, 0);
        assert_eq!(
            ir.model.features[1].id.as_str(),
            "table-a:feature#equations"
        );
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
        for (parameter, value) in ir.model.parameters.iter_mut().zip([7.0, 14.0, 5.0, 10.0]) {
            parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(value),
            ));
        }
        assert!(crate::decode::incomplete_expression_parameters(&ir).is_empty());

        let mut inconsistent = ir.clone();
        inconsistent.model.parameters[1].value = Some(
            cadmpeg_ir::features::ParameterValue::Length(cadmpeg_ir::features::Length(1.0)),
        );
        assert_eq!(
            crate::decode::incomplete_expression_parameters(&inconsistent),
            [inconsistent.model.parameters[1].id.clone()].into()
        );

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
                parameters: BTreeMap::default(),
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
        for (parameter, value) in ir.model.parameters.iter_mut().zip([7.0, 14.0, 1.0, 1.0]) {
            parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(value),
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
        let mut ir = cadmpeg_ir::CadIr::empty();
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
                cadmpeg_ir::features::ParameterId("nx:test:parameter#20".into()),
                cadmpeg_ir::features::ParameterId("nx:test:parameter#20".into()),
            ],
        );

        assert_eq!(ir.model.features[0].ordinal, 0);
        assert_eq!(
            dependencies,
            [ir.model.parameters[0].owner.clone().unwrap()]
        );
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
    fn om_offset_store_values_precede_unique_product_anchor() {
        let mut bytes = vec![0, 0];
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&0x1020u32.to_le_bytes());
        bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
        assert_eq!(
            crate::om::offset_store_control_form(&bytes, None),
            Some(crate::om::OffsetStoreControlForm::ProductAnchored {
                leading_value: Some((2, 0)),
                values: vec![7, 0x1020],
            })
        );

        let mut nonzero_leading = vec![0x34, 0x12, 0x00];
        nonzero_leading.extend_from_slice(&7u32.to_le_bytes());
        nonzero_leading.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
        assert_eq!(
            crate::om::offset_store_control_form(&nonzero_leading, None),
            Some(crate::om::OffsetStoreControlForm::ProductAnchored {
                leading_value: Some((3, 0x1234)),
                values: vec![7],
            })
        );

        let mut duplicate = bytes;
        duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
        assert!(crate::om::offset_store_control_form(&duplicate, None).is_none());
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
        let container = container::scan_bytes(file).expect("required invariant");

        assert!(super::object_records(&container).is_empty());
        let blocks = super::data_blocks(&container);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].block_ordinal, 0);
        assert_eq!(blocks[0].role, super::DataBlockRole::Control);
        assert_eq!(blocks[1].role, super::DataBlockRole::Column);
        assert!(blocks[0].byte_len > 0);
        assert!(blocks[0].stable_identity.is_some());
        let forms = super::data_block_control_forms(&container);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].data_block, blocks[0].id);
        assert_eq!(forms[0].kind, super::DataBlockControlFormKind::ZeroPrefixed);
        assert_eq!(forms[0].value_count, 2);
        assert_eq!(forms[0].leading_value_width, None);
        assert_eq!(forms[0].leading_value, None);
        assert_eq!(forms[0].byte_len, blocks[0].byte_len);
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
        assert_eq!(classes[0].class_name.as_deref(), Some("UGS::ModlFeature"));
        assert_eq!(
            classes[0].class_definition.as_deref(),
            Some("nx:om-entry-0:class#8")
        );
        assert!(super::string_values(&container).is_empty());
        assert!(super::object_references(&container).is_empty());
        let expressions = super::expressions(&container);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].object_id, None);
        assert_eq!(expressions[0].record, None);
    }

    #[test]
    fn stable_data_block_identity_excludes_position_and_scopes_role() {
        let bytes = [0x01, 0x02, 0x03];
        let identity = super::stable_data_block_identity(
            "/Root/UG_PART/UG_PART",
            super::DataBlockRole::Column,
            &bytes,
        );
        assert_eq!(
            identity,
            super::stable_data_block_identity(
                "/Root/UG_PART/UG_PART",
                super::DataBlockRole::Column,
                &bytes,
            )
        );
        assert_ne!(
            identity,
            super::stable_data_block_identity(
                "/Root/UG_PART/UG_PART",
                super::DataBlockRole::Control,
                &bytes,
            )
        );
        assert_ne!(
            identity,
            super::stable_data_block_identity("/Root/other", super::DataBlockRole::Column, &bytes)
        );
    }

    #[test]
    fn native_catalog_classifies_product_anchored_control_atomically() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            offset_only_indexed_om_section_with_index_values(),
        )]);
        let container = container::scan_bytes(file).expect("required invariant");

        let forms = super::data_block_control_forms(&container);
        assert_eq!(forms.len(), 1);
        assert_eq!(
            forms[0].kind,
            super::DataBlockControlFormKind::ProductAnchored
        );
        assert_eq!(forms[0].value_count, 2);
        assert_eq!(forms[0].leading_value_width, Some(2));
        assert_eq!(forms[0].leading_value, Some(0));
        assert!(super::data_block_control_values(&container).is_empty());
        assert_eq!(super::data_block_control_index_values(&container).len(), 2);
    }

    #[test]
    fn offset_store_class_identities_span_ordered_registries() {
        let mut store =
            offset_only_indexed_om_section_with_control(&[0, 1, 0, 0, 0, 10, 0, 0, 0, 5, 0, 0]);
        store.extend_from_slice(&size_framed_om_section());
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
        let container = container::scan_bytes(file).expect("required invariant");

        let classes = super::data_block_control_class_references(&container);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].class_ordinal, 1);
        assert_eq!(
            classes[0].class_name.as_deref(),
            Some("UGS::FEATURE_RECORD")
        );
        assert!(classes[0].class_definition.is_some());
    }

    #[test]
    fn native_abr_lane_resolves_nullable_slots_within_its_offset_store() {
        let mut store = offset_only_indexed_om_section();
        let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
        let end_at = index_start + 3 * 4;
        let end = u32::from_le_bytes(
            store[end_at..end_at + 4]
                .try_into()
                .expect("required invariant"),
        ) as usize;
        let mut lane = vec![0x11, 0x02];
        lane.extend_from_slice(&[0xff; 15]);
        lane.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
        store.splice(end..end, lane.iter().copied());
        store[end_at..end_at + 4].copy_from_slice(&((end + lane.len()) as u32).to_le_bytes());
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
        let container = container::scan_bytes(file).expect("required invariant");

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
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");
        let expressions = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::Expression>("expressions")
            .expect("required invariant");
        assert_eq!(
            result
                .ir()
                .native
                .namespace("nx")
                .expect("required invariant")
                .version(),
            189
        );
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
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ExpressionDeclaration>("expression_declarations")
            .expect("required invariant");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].object_id, 0x102);
        assert_eq!(declarations[0].parameter_index, 8);
        assert_eq!(declarations[0].literal.as_deref(), Some("120"));
        assert_eq!(
            expressions[0].declaration.as_deref(),
            Some(declarations[0].id.as_str())
        );
        let parameter = result
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == expressions[0].name)
            .expect("required invariant");
        assert_eq!(
            parameter.properties.get("declaration"),
            Some(&declarations[0].id)
        );
        assert_eq!(
            parameter.properties.get("declaration_object_id"),
            Some(&"258".to_string())
        );
        let om_records = result
            .source_fidelity()
            .retained_records
            .iter()
            .filter(|record| record.id().starts_with("nx:om-section-"))
            .collect::<Vec<_>>();
        assert_eq!(om_records.len(), 2);
        assert!(om_records.iter().all(|record| {
            record.data().is_some_and(|data| {
                data.len() as u64 == record.byte_len()
                    && cadmpeg_ir::hash::sha256_hex(data) == record.sha256()
            })
        }));
        let object_records = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ObjectRecord>("object_records")
            .expect("required invariant");
        assert_eq!(object_records.len(), 2);
        let headers = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::StoreHeader>("store_headers")
            .expect("required invariant");
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
        assert_eq!(object_records[1].byte_len, om_records[1].byte_len());
        assert_eq!(object_records[1].sha256, om_records[1].sha256());
        assert_eq!(
            object_records[1].dependencies,
            vec![object_records[0].id.clone()]
        );
        assert_eq!(
            object_records[0].dependents,
            vec![object_records[1].id.clone()]
        );
        let strings = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::StringValue>("string_values")
            .expect("required invariant");
        assert_eq!(strings.len(), 1);
        assert_eq!(strings[0].record, object_records[1].id);
        assert_eq!(strings[0].object_id, Some(0x102));
        assert_eq!(strings[0].value, "SKETCH_001");
        let references = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ObjectReference>("object_references")
            .expect("required invariant");
        assert_eq!(references.len(), 3);
        assert_eq!(references[0].record, object_records[1].id);
        assert_eq!(references[0].object_id, Some(0x102));
        assert_eq!(references[0].value, 0x1234_5678);
        assert_eq!(references[0].target_record, None);
        assert_eq!(references[1].kind, super::ObjectReferenceKind::Tagged28);
        assert_eq!(references[1].value, 0x0abc_def0);
        assert_eq!(references[1].target_record, None);
        assert_eq!(
            references[2].kind,
            super::ObjectReferenceKind::RecordOrdinal16
        );
        assert_eq!(references[2].value, 0);
        assert_eq!(
            references[2].target_record.as_ref(),
            Some(&object_records[0].id)
        );
        let handles = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::PersistentHandle>("persistent_handles")
            .expect("required invariant");
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].value, 0x1234_5678);
        assert_eq!(handles[0].records, vec![object_records[1].id.clone()]);
        assert_eq!(handles[0].occurrence_count, 1);
        assert!(handles[0].external_records.is_empty());
        assert_eq!(result.ir().model.features.len(), 1);
        assert!(matches!(
            result.ir().model.features[0].definition,
            cadmpeg_ir::features::FeatureDefinition::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations,
                ..
            }
        ));
        assert_eq!(result.ir().model.features[0].suppressed, Some(false));
        assert_eq!(result.ir().model.parameters.len(), 1);
        assert_eq!(result.ir().model.parameters[0].expression, "120");
        let parameter = &result.ir().model.parameters[0];
        assert_eq!(parameter.name, expressions[0].name);
        assert!(matches!(
            parameter.value,
            Some(cadmpeg_ir::features::ParameterValue::Angle(
                cadmpeg_ir::features::Angle(value)
            )) if value == 120_f64.to_radians()
        ));
        assert_eq!(parameter.native_ref.as_ref(), Some(&expressions[0].id));
        let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
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
            .expect("required invariant");
        assert!(super::parse_part_attributes(&malformed, 7, "/Root/part/attrs", 100).is_none());
    }

    #[test]
    fn decode_retains_length_framed_nx_class_definition() {
        let mut cur = Cursor::new(prt_with_indexed_om_section());
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");
        let classes = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ClassDefinition>("class_definitions")
            .expect("required invariant");
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "UGS::EXP_expression");
        assert_eq!(classes[0].ordinal, 0);
        assert_eq!(classes[0].trailing_code, 0x81);
        assert_eq!(classes[0].source_entry, "/Root/UG_PART/UG_PART");
    }

    #[test]
    fn decode_retains_length_framed_nx_field_definitions() {
        let mut cur = Cursor::new(prt_with_size_framed_om_section());
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");
        let fields = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::FieldDefinition>("field_definitions")
            .expect("required invariant");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "m_target");
        assert_eq!(fields[0].ordinal, 0);
        assert_eq!(fields[0].registry_storage_code, Some(2));
        assert_eq!(fields[0].registry_owner_class, Some(2));
        assert_eq!(fields[0].registry_suffix, [0x01, 0x02]);
        assert_eq!(fields[0].layout_prefix, Vec::<u8>::new());
        assert_eq!(fields[0].schema_fingerprint, None);
        assert_eq!(fields[0].layout_terminal, None);
        assert_eq!(fields[1].name, "m_tools");
        assert_eq!(fields[1].trailing_code, 0x81);
        assert!(fields[1].registry_suffix.is_empty());
        assert_eq!(fields[1].source_entry, "/Root/UG_PART/UG_PART");
        let (prefix, fingerprint, terminal) = super::registry_layout_fields(&[
            0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
        ]);
        assert_eq!(prefix, [0x81, 0x21]);
        assert_eq!(
            fingerprint,
            Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
        );
        assert_eq!(terminal, Some(0x06));
        let classes = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ClassDefinition>("class_definitions")
            .expect("required invariant");
        assert_eq!(classes[0].layout_prefix, &[0x81, 0x21]);
        assert_eq!(
            classes[0].schema_fingerprint,
            Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
        );
        assert_eq!(classes[0].layout_terminal, Some(0x06));
    }

    #[test]
    fn class_registry_metadata_requires_a_complete_tail() {
        let legacy_definition = crate::om::TypeDefinition {
            offset: 0,
            name: "UGS::FEATURE_RECORD",
            trailing_code: 0xa0,
            registry_suffix: &[
                0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ],
        };

        let legacy = super::class_registry_fields(&legacy_definition);
        assert_eq!(legacy.storage_code, None);
        assert_eq!(legacy.base_class, None);
        assert_eq!(legacy.reference, None);
        assert_eq!(
            legacy.schema_fingerprint,
            Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
        );
        assert_eq!(legacy.layout_terminal, Some(0x06));

        let complete_definition = crate::om::TypeDefinition {
            offset: 0,
            name: "UGS::FEATURE_RECORD",
            trailing_code: 0x38,
            registry_suffix: &[0x05, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x02],
        };
        let complete = super::class_registry_fields(&complete_definition);
        assert_eq!(complete.storage_code, Some(0x38));
        assert_eq!(complete.base_class, Some(0x05));
        assert_eq!(complete.reference, Some(0x02));
        assert_eq!(
            complete.schema_fingerprint,
            Some([0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80])
        );
        assert_eq!(complete.layout_terminal, None);
    }

    #[test]
    fn decode_retains_nx_arrangement_configurations() {
        let mut cur = Cursor::new(prt_with_arrangements());
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");
        let configurations = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::Configuration>("configurations")
            .expect("required invariant");
        assert_eq!(configurations.len(), 2);
        assert_eq!(configurations[0].name, "Model");
        assert!(configurations[0].is_default);
        assert_eq!(configurations[1].name, "Exploded");
        assert!(!configurations[1].is_default);
        assert_eq!(result.ir().model.configurations.len(), 2);
        assert_eq!(result.ir().model.configurations[0].ordinal, 0);
        assert_eq!(result.ir().model.configurations[0].source_index, Some(0));
        assert_eq!(result.ir().model.configurations[0].name, "Model");
        assert!(result.ir().model.configurations[0].active);
        assert_eq!(
            result.ir().model.configurations[0].bodies.resolved(),
            Some(
                result
                    .ir()
                    .model
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
                    .collect::<Vec<_>>()
                    .as_slice()
            )
        );
        assert_eq!(result.ir().model.configurations[1].ordinal, 1);
        assert_eq!(result.ir().model.configurations[1].name, "Exploded");
        assert!(!result.ir().model.configurations[1].active);
        assert!(result.ir().model.configurations[1].bodies.is_unresolved());
        let uses = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ConfigurationAttributeUse>("configuration_attribute_uses")
            .expect("required invariant");
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].configuration, configurations[0].id);
        assert_eq!(uses[0].name, "Model");
        assert_eq!(
            result.ir().model.configurations[0].properties["active_attribute_use"],
            uses[0].id
        );
        let attributes = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::PartAttribute>("part_attributes")
            .expect("required invariant");
        let mut mismatch = attributes.clone();
        mismatch[0].value = "Other".to_string();
        assert!(super::configuration_attribute_uses(&configurations, &mismatch).is_empty());
        let mut duplicate = attributes.clone();
        duplicate.push(attributes[0].clone());
        assert!(super::configuration_attribute_uses(&configurations, &duplicate).is_empty());
        let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    }

    #[test]
    fn nx_neutral_active_configuration_requires_the_exact_attribute_join() {
        for active_name in [None, Some("Other")] {
            let mut cur = Cursor::new(prt_with_arrangement_attribute(active_name));
            let result = NxCodec
                .decode(&mut cur, &DecodeOptions::default())
                .expect("required invariant");
            let native = result
                .ir()
                .native
                .namespace("nx")
                .expect("required invariant")
                .arena_as::<super::Configuration>("configurations")
                .expect("required invariant");
            assert!(native[0].is_default);
            assert!(
                result
                    .ir()
                    .model
                    .configurations
                    .iter()
                    .all(|configuration| !configuration.active
                        && configuration.bodies.is_unresolved())
            );
        }
    }
    mod material_and_external_records;
}

#[cfg(test)]
mod rmfastload;

#[cfg(test)]
mod object_record_identity_tests {

    use crate::test_support::prt_with_indexed_om_section;

    #[test]
    fn stable_object_record_identity_excludes_position_and_scopes_entry() {
        let bytes = [0x04, 0x05, 0x06];
        let identity = super::stable_object_record_identity("/Root/UG_PART/UG_PART", &bytes);
        assert_eq!(
            identity,
            super::stable_object_record_identity("/Root/UG_PART/UG_PART", &bytes)
        );
        assert_ne!(
            identity,
            super::stable_object_record_identity("/Root/other", &bytes)
        );
        assert_ne!(
            identity,
            super::stable_object_record_identity("/Root/UG_PART/UG_PART", &[0x04, 0x05, 0x07])
        );
    }

    #[test]
    fn unique_indexed_object_records_receive_stable_identities() {
        let container = crate::container::scan_bytes(prt_with_indexed_om_section())
            .expect("required invariant");
        let records = super::object_records(&container);
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record.stable_identity.is_some()));
        assert_ne!(records[0].stable_identity, records[1].stable_identity);
    }

    #[test]
    fn graph_identity_ignores_same_section_record_reordering() {
        let first: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x01, 0xa0];
        let second: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x00, 0xb0];
        let original = [first, second];

        let reordered_first: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x01, 0xb0];
        let reordered_second: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x00, 0xa0];
        let reordered = [reordered_first, reordered_second];

        let original_identities = super::stable_object_record_identities("/entry", &original);
        let reordered_identities = super::stable_object_record_identities("/entry", &reordered);
        assert_eq!(original_identities[0], reordered_identities[1]);
        assert_eq!(original_identities[1], reordered_identities[0]);

        let unrelated: &[u8] = &[0xd0];
        let with_unrelated = [original[0], original[1], unrelated];
        let with_unrelated_identities =
            super::stable_object_record_identities("/entry", &with_unrelated);
        assert_eq!(original_identities[0], with_unrelated_identities[0]);
        assert_eq!(original_identities[1], with_unrelated_identities[1]);

        let changed = [reordered_first, &[0x01, 0x02, 0x90, 0x00, 0x00, 0xc0][..]];
        let changed_identities = super::stable_object_record_identities("/entry", &changed);
        assert_ne!(original_identities[0], changed_identities[1]);
    }
}
