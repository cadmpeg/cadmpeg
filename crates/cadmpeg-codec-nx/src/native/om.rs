// SPDX-License-Identifier: Apache-2.0
//! Object-model, data-block, expression, and external-reference extractors and record types.

#[cfg(test)]
use crate::decode::feature_completeness;

#[allow(clippy::wildcard_imports)]
use super::*;
use crate::om::control_leading_value::ControlLeadingValue;
use crate::om::reference_value::{DirectReference, RecordReference};
use crate::om::state_message::StateMessage;
use crate::om::state_table::StateTableEntry;
use crate::printable_string::PrintableString;
pub(crate) mod journal_group;
pub(crate) mod material_texture;
pub(crate) mod object_uuid;
mod reference_wire;
mod state_index_wire;
use journal_group::OmOperationStateJournalGroup;
use material_texture::MaterialTextureAsset;

pub(crate) mod column_row;
pub(crate) mod compact_lane;
pub(crate) mod creation_display;
pub(crate) mod display_color;
use column_row::{DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
mod membership_wire;
use crate::container::extref_handles::ExtrefHandles;
use crate::container::extref_slot::ExtrefSlot;
use crate::container::membership::ObjectIdMembers;
use crate::om::color::{ColorComponent, PaletteIndex, BACKGROUND_NAME, PALETTE_SIZE};
mod color_wire;
pub(crate) mod column_index;
use column_index::ColumnIndexRows;

use crate::native::segments::segment_om_links;
use crate::om::parameter_name::ParameterName;
pub(crate) mod roll_forward;
use crate::om::IndexedStore;
use roll_forward::OmRollForwardStateGroup;
pub(crate) mod state_slot_lane;
use state_slot_lane::OmOperationStateSlotLane;
pub(crate) mod state_status;
use state_status::OmOperationStateStatus;

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
    pub product_version: crate::om::product::ProductText<String>,
    /// Exact record-area byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete pointed record area.
    pub sha256: String,
    /// Absolute file offset of the first control word.
    pub source_offset: u64,
}

/// One complete row retained from an audit-trail record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "state_index_wire::OmAuditTrailRowWire",
    into = "state_index_wire::OmAuditTrailRowWire"
)]
pub struct OmAuditTrailRow {
    /// Globally unique audit-row identity.
    pub id: String,
    /// Owning audit-trail section link.
    pub section_link: String,
    /// Exact framed audit content.
    record: crate::om::audit::AuditRecord,
    /// Directory entry containing the audit-trail section.
    pub source_entry: String,
    /// Absolute file offset of the row's opening `04` marker.
    source_offset: u64,
}

impl OmAuditTrailRow {
    fn new(
        id: String,
        section_link: String,
        record: crate::om::audit::AuditRecord,
        source_entry: String,
        source_offset: u64,
    ) -> Option<Self> {
        source_offset.checked_add(record.byte_len() as u64)?;
        Some(Self {
            id,
            section_link,
            record,
            source_entry,
            source_offset,
        })
    }

    pub fn record(&self) -> crate::om::audit::AuditRecord {
        self.record
    }
    pub fn source_offset(&self) -> u64 {
        self.source_offset
    }
    pub fn end_offset(&self) -> u64 {
        self.source_offset + self.record.byte_len() as u64
    }
}

/// One row from the feature-history operation-state counter map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "state_index_wire::OmOperationStateCounterWire",
    into = "state_index_wire::OmOperationStateCounterWire"
)]
pub struct OmOperationStateCounter {
    /// Globally unique counter-row identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based row ordinal within the section's counter map.
    pub ordinal: u32,
    /// Complete counter row with derived index position.
    pub frame: crate::om::state_counter::StateCounter,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
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
    /// Diagnostic payload.
    #[serde(flatten)]
    pub body: StateMessage<String>,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the opening `03` marker.
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
            let bytes = section.record_area?.bytes;
            let entry_offset = link.section_offset.checked_sub(section.offset as u64)?;
            let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(OmRecordArea {
                id: format!("nx:om-record-areas:area#{section_key}-{}", header.offset),
                section_link: link.id,
                schema_role: link.schema_role,
                control_words: header.control_words,
                product_version: header.product.value.into_owned(),
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
                    let record = row.record();
                    let ordinal = record.ordinal.value();
                    OmAuditTrailRow::new(
                        format!("nx:audit-trail:row#{section_key}-{ordinal:010}"),
                        link.id.clone(),
                        record,
                        entry.name.clone(),
                        entry_offset.checked_add(row.offset() as u64)?,
                    )
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
            map.into_rows()
                .enumerate()
                .filter_map(move |(ordinal, row)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    Some(OmOperationStateCounter {
                        id: format!(
                            "nx:feature-history:operation-state-counter#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        frame: row.into_absolute(entry_offset)?,
                        source_entry: entry.name.clone(),
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
                    Some(OmOperationStateJournalGroup {
                        id: format!(
                            "nx:feature-history:operation-state-journal-group#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        frame: group.into_absolute(entry_offset)?,
                        source_entry: entry.name.clone(),
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
            let table_end_offset = entry_offset + table.end_offset() as u64;
            let table_footer = table.footer();
            table
                .into_groups()
                .into_iter()
                .enumerate()
                .filter_map(move |(ordinal, group)| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    Some(OmRollForwardStateGroup {
                        id: format!(
                            "nx:feature-history:roll-forward-state-group#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        frame: group.into_absolute(entry_offset)?,
                        table_footer,
                        source_entry: entry.name.clone(),
                        table_end_offset,
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
                        body: message.body().into_owned(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + message.offset() as u64,
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
                .into_entries()
                .filter_map(|(offset, entry)| match entry {
                    StateTableEntry::Status(row) => Some((offset, row)),
                    StateTableEntry::Slots(_) => None,
                })
                .enumerate()
                .filter_map(move |(ordinal, (offset, row))| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    OmOperationStateStatus::new(
                        format!(
                            "nx:feature-history:operation-state-status#{section_key}-{ordinal:010}"
                        ),
                        link.id.clone(),
                        ordinal,
                        row.into_owned(),
                        entry.name.clone(),
                        entry_offset.checked_add(offset as u64)?,
                    )
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
                .into_entries()
                .filter_map(|(offset, entry)| match entry {
                    StateTableEntry::Status(_) => None,
                    StateTableEntry::Slots(slots) => Some((offset, slots)),
                })
                .enumerate()
                .filter_map(move |(ordinal, (offset, slots))| {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    Some(OmOperationStateSlotLane {
                        id: format!(
                            "nx:feature-history:operation-state-slot-lane#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal,
                        frame: crate::om::state_slot_lane::StateSlotLane::new(entry_offset.checked_add(offset as u64)?, slots).ok()?,
                        source_entry: entry.name.clone(),
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
#[serde(
    try_from = "ExpressionDeclarationWire",
    into = "ExpressionDeclarationWire"
)]
pub struct ExpressionDeclaration {
    /// Globally unique declaration identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: u32,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Exact NX parameter name.
    pub name: ParameterName<String, u32>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    /// Directory entry containing the declaration record.
    pub source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct ExpressionDeclarationWire {
    /// Globally unique declaration identity.
    id: String,
    /// Persistent OM object identifier.
    object_id: u32,
    /// Owning entry in the native OM record directory.
    record: String,
    /// Exact NX parameter name.
    name: String,
    /// Decimal source parameter identifier following `p`.
    parameter_index: u32,
    /// Qualified role following the parameter identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    qualifier: Option<String>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    literal: Option<String>,
    /// Directory entry containing the declaration record.
    source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    source_offset: u64,
}

impl From<ExpressionDeclaration> for ExpressionDeclarationWire {
    fn from(value: ExpressionDeclaration) -> Self {
        Self {
            parameter_index: value.name.index(),
            qualifier: value.name.qualifier().map(str::to_string),
            name: value.name.into_spelling(),
            id: value.id,
            object_id: value.object_id,
            record: value.record,
            literal: value.literal,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<ExpressionDeclarationWire> for ExpressionDeclaration {
    type Error = String;
    fn try_from(wire: ExpressionDeclarationWire) -> Result<Self, Self::Error> {
        let name = ParameterName::<_, u32>::parse(wire.name)
            .ok_or("name must use canonical parameter syntax")?;
        if name.index() != wire.parameter_index || name.qualifier() != wire.qualifier.as_deref() {
            return Err("parameter_index and qualifier must match name".into());
        }
        Ok(Self {
            name,
            id: wire.id,
            object_id: wire.object_id,
            record: wire.record,
            literal: wire.literal,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        })
    }
}

/// Explicit numeric expression serialized in one NX OM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ExpressionWire", into = "ExpressionWire")]
pub struct Expression {
    /// Globally unique native-record identity.
    pub id: String,
    /// Externally bounded OM record and its persistent identity.
    pub owner: Option<ExpressionOwner>,
    /// Exact-name declaration record for this parameter, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<String>,
    /// NX parameter name.
    pub name: ParameterName<String>,
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
    pub source_table: cadmpeg_ir::NonEmptyString,
    /// Absolute file offset of the expression text.
    pub source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionOwner {
    pub object_id: u32,
    pub record: String,
}

#[derive(Serialize, Deserialize)]
struct ExpressionWire {
    /// Globally unique native-record identity.
    id: String,
    /// Persistent OM object identifier.
    object_id: Option<u32>,
    /// Owning entry in the native OM record directory, when externally bounded.
    record: Option<String>,
    /// Exact-name declaration record for this parameter, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    declaration: Option<String>,
    /// NX parameter name.
    name: String,
    /// Decimal source parameter identifier following the leading `p`.
    parameter_index: Option<u32>,
    /// Qualified role following the parameter identifier.
    qualifier: Option<String>,
    /// Declared native unit.
    unit: ExpressionUnit,
    /// Exact serialized expression text.
    expression: String,
    /// Finite numeric value after context-free and dependency-graph evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Self-contained expression table selected by the nearest preceding table marker.
    #[serde(default)]
    source_table: String,
    /// Absolute file offset of the expression text.
    source_offset: u64,
}

impl From<Expression> for ExpressionWire {
    fn from(value: Expression) -> Self {
        let (object_id, record) = value.owner.map_or((None, None), |owner| {
            (Some(owner.object_id), Some(owner.record))
        });
        Self {
            object_id,
            record,
            id: value.id,
            declaration: value.declaration,
            parameter_index: value.name.index(),
            qualifier: value.name.qualifier().map(str::to_string),
            name: value.name.into_spelling(),
            unit: value.unit,
            expression: value.expression,
            value: value.value,
            source_entry: value.source_entry,
            source_table: value.source_table.as_str().to_owned(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<ExpressionWire> for Expression {
    type Error = String;
    fn try_from(wire: ExpressionWire) -> Result<Self, Self::Error> {
        let name = ParameterName::new(wire.name);
        if name.index() != wire.parameter_index || name.qualifier() != wire.qualifier.as_deref() {
            return Err("parameter_index and qualifier must match name".into());
        }
        let owner = match (wire.object_id, wire.record) {
            (None, None) => None,
            (Some(object_id), Some(record)) => Some(ExpressionOwner { object_id, record }),
            _ => return Err("object_id and record are present together".into()),
        };
        Ok(Self {
            owner,
            id: wire.id,
            declaration: wire.declaration,
            name,
            unit: wire.unit,
            expression: wire.expression,
            value: wire.value,
            source_entry: wire.source_entry,
            source_table: cadmpeg_ir::NonEmptyString::new(wire.source_table)
                .ok_or("source_table must not be empty")?,
            source_offset: wire.source_offset,
        })
    }
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
    ParameterName::<_, u32>::parse(name).map(|_| end)
}

/// Length-framed class definition from an NX OM type registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ClassDefinitionWire", into = "ClassDefinitionWire")]
pub struct ClassDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Registered `UGS::` class name.
    pub name: String,
    /// Zero-based declaration ordinal used as class identity.
    pub ordinal: u32,
    /// First registry-token byte serialized after the class name (legacy field name).
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the definition's length byte.
    pub source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClassDefinitionWire {
    /// Globally unique native-record identity.
    id: String,
    /// Registered `UGS::` class name.
    name: String,
    /// Zero-based declaration ordinal used as class identity.
    ordinal: u32,
    /// First registry-token byte serialized after the class name (legacy field name).
    trailing_code: u8,
    /// Decoded storage token from the complete class registry tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry_storage_code: Option<u32>,
    /// One-based base-class ordinal from the complete class registry tail.
    /// Zero denotes the registry root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry_base_class: Option<u32>,
    /// One-based reference-list ordinal from the complete class registry tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry_reference: Option<u32>,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layout_prefix: Vec<u8>,
    /// Stable eight-byte class fingerprint in a framed registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_fingerprint: Option<[u8; 8]>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout_terminal: Option<u8>,
    /// Absolute file offset of the containing OM section base.
    section_offset: u64,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the definition's length byte.
    source_offset: u64,
}

impl From<ClassDefinition> for ClassDefinitionWire {
    fn from(value: ClassDefinition) -> Self {
        let tail: Vec<_> = std::iter::once(value.trailing_code)
            .chain(value.registry_suffix.iter().copied())
            .collect();
        let registry = crate::om::registry::class_registry_layout(&tail);
        let layout = registry_layout(&tail[1..]);
        Self {
            id: value.id,
            name: value.name,
            ordinal: value.ordinal,
            trailing_code: value.trailing_code,
            registry_storage_code: registry.map(|layout| layout.storage_code.value()),
            registry_base_class: registry
                .map(|layout| layout.base_class.map_or(0, std::num::NonZeroU32::get)),
            registry_reference: registry.map(|layout| layout.reference.get()),
            registry_suffix: value.registry_suffix,
            layout_prefix: layout
                .as_ref()
                .map_or_else(Vec::new, |layout| layout.prefix.to_vec()),
            schema_fingerprint: registry
                .map(|layout| layout.schema_fingerprint)
                .or_else(|| layout.as_ref().map(|layout| layout.fingerprint)),
            layout_terminal: layout.as_ref().map(|layout| layout.terminal),
            section_offset: value.section_offset,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<ClassDefinitionWire> for ClassDefinition {
    type Error = String;
    fn try_from(wire: ClassDefinitionWire) -> Result<Self, Self::Error> {
        let value = Self {
            id: wire.id,
            name: wire.name,
            ordinal: wire.ordinal,
            trailing_code: wire.trailing_code,
            registry_suffix: wire.registry_suffix,
            section_offset: wire.section_offset,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        };
        let expected = ClassDefinitionWire::from(value.clone());
        if wire.registry_storage_code != expected.registry_storage_code {
            return Err(
                "ClassDefinition registry_storage_code disagrees with registry bytes".into(),
            );
        }
        if wire.registry_base_class != expected.registry_base_class {
            return Err("ClassDefinition registry_base_class disagrees with registry bytes".into());
        }
        if wire.registry_reference != expected.registry_reference {
            return Err("ClassDefinition registry_reference disagrees with registry bytes".into());
        }
        if wire.layout_prefix != expected.layout_prefix {
            return Err("ClassDefinition layout_prefix disagrees with registry bytes".into());
        }
        if wire.schema_fingerprint != expected.schema_fingerprint {
            return Err("ClassDefinition schema_fingerprint disagrees with registry bytes".into());
        }
        if wire.layout_terminal != expected.layout_terminal {
            return Err("ClassDefinition layout_terminal disagrees with registry bytes".into());
        }
        Ok(value)
    }
}

/// Member declaration from an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FieldDefinitionWire", into = "FieldDefinitionWire")]
pub struct FieldDefinition {
    /// Globally unique declaration identity.
    pub id: String,
    /// Registered `m_` member name.
    pub name: String,
    /// Zero-based declaration ordinal within its section.
    pub ordinal: u32,
    /// First registry-token byte serialized immediately after the name (legacy field name).
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FieldDefinitionWire {
    /// Globally unique declaration identity.
    id: String,
    /// Registered `m_` member name.
    name: String,
    /// Zero-based declaration ordinal within its section.
    ordinal: u32,
    /// First registry-token byte serialized immediately after the name (legacy field name).
    trailing_code: u8,
    /// Decoded storage token from the complete member registry head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry_storage_code: Option<u32>,
    /// One-based declaring-class ordinal from the complete member registry head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry_owner_class: Option<u32>,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layout_prefix: Vec<u8>,
    /// Stable eight-byte field fingerprint in a framed registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_fingerprint: Option<[u8; 8]>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout_terminal: Option<u8>,
    /// Absolute file offset of the containing OM section signature.
    section_offset: u64,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the declaration length byte.
    source_offset: u64,
}

impl From<FieldDefinition> for FieldDefinitionWire {
    fn from(value: FieldDefinition) -> Self {
        let tail: Vec<_> = std::iter::once(value.trailing_code)
            .chain(value.registry_suffix.iter().copied())
            .collect();
        let registry = crate::om::registry::field_registry_layout(&tail);
        let layout = registry_layout(&tail[1..]);
        Self {
            id: value.id,
            name: value.name,
            ordinal: value.ordinal,
            trailing_code: value.trailing_code,
            registry_storage_code: registry.map(|layout| layout.storage_code.value()),
            registry_owner_class: registry.map(|layout| layout.owner_class.get()),
            registry_suffix: value.registry_suffix,
            layout_prefix: layout
                .as_ref()
                .map_or_else(Vec::new, |layout| layout.prefix.to_vec()),
            schema_fingerprint: layout.as_ref().map(|layout| layout.fingerprint),
            layout_terminal: layout.as_ref().map(|layout| layout.terminal),
            section_offset: value.section_offset,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FieldDefinitionWire> for FieldDefinition {
    type Error = String;
    fn try_from(wire: FieldDefinitionWire) -> Result<Self, Self::Error> {
        let value = Self {
            id: wire.id,
            name: wire.name,
            ordinal: wire.ordinal,
            trailing_code: wire.trailing_code,
            registry_suffix: wire.registry_suffix,
            section_offset: wire.section_offset,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        };
        let expected = FieldDefinitionWire::from(value.clone());
        if wire.registry_storage_code != expected.registry_storage_code {
            return Err(
                "FieldDefinition registry_storage_code disagrees with registry bytes".into(),
            );
        }
        if wire.registry_owner_class != expected.registry_owner_class {
            return Err(
                "FieldDefinition registry_owner_class disagrees with registry bytes".into(),
            );
        }
        if wire.layout_prefix != expected.layout_prefix {
            return Err("FieldDefinition layout_prefix disagrees with registry bytes".into());
        }
        if wire.schema_fingerprint != expected.schema_fingerprint {
            return Err("FieldDefinition schema_fingerprint disagrees with registry bytes".into());
        }
        if wire.layout_terminal != expected.layout_terminal {
            return Err("FieldDefinition layout_terminal disagrees with registry bytes".into());
        }
        Ok(value)
    }
}

/// Directory entry for one externally bounded NX OM entity record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ObjectRecordWire", into = "ObjectRecordWire")]
pub struct ObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Persistent OM object identifier and the offset of its table word.
    pub object_id: (u32, u64),
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

#[derive(Serialize, Deserialize)]
struct ObjectRecordWire {
    id: String,
    object_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_id_source_offset: Option<u64>,
    section_ordinal: u32,
    record_ordinal: u32,
    section_offset: u64,
    byte_len: u64,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stable_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependents: Vec<String>,
    source_entry: String,
    source_offset: u64,
}

impl From<ObjectRecord> for ObjectRecordWire {
    fn from(value: ObjectRecord) -> Self {
        let (id, offset) = value.object_id;
        let (object_id, object_id_source_offset) = (Some(id), Some(offset));
        Self {
            id: value.id,
            object_id,
            object_id_source_offset,
            section_ordinal: value.section_ordinal,
            record_ordinal: value.record_ordinal,
            section_offset: value.section_offset,
            byte_len: value.byte_len,
            sha256: value.sha256,
            stable_identity: value.stable_identity,
            dependencies: value.dependencies,
            dependents: value.dependents,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<ObjectRecordWire> for ObjectRecord {
    type Error = String;

    fn try_from(wire: ObjectRecordWire) -> Result<Self, Self::Error> {
        let object_id = match (wire.object_id, wire.object_id_source_offset) {
            (Some(id), Some(offset)) => (id, offset),
            _ => {
                return Err(
                    "object record object_id and object_id_source_offset are required together"
                        .to_owned(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            object_id,
            section_ordinal: wire.section_ordinal,
            record_ordinal: wire.record_ordinal,
            section_offset: wire.section_offset,
            byte_len: wire.byte_len,
            sha256: wire.sha256,
            stable_identity: wire.stable_identity,
            dependencies: wire.dependencies,
            dependents: wire.dependents,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        })
    }
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
                .map(|reference| (reference.offset, usize::from(reference.value)))
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
#[serde(
    try_from = "membership_wire::TableWire",
    into = "membership_wire::TableWire"
)]
pub struct RmFastLoadObjectIdTable {
    /// Globally unique table identity.
    pub id: String,
    /// Ordered members in the native `rmfastload_object_ids` arena.
    pub members: ObjectIdMembers<String>,
    /// Directory entry containing the table.
    pub source_entry: String,
    /// Absolute file offset of the `UGS::Solid::Topol` registry marker.
    pub registry_source_offset: u64,
    /// Absolute file offset of the four-byte count word.
    pub source_offset: u64,
}

/// One fixed-width active-object membership word from `RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "membership_wire::MemberWire",
    into = "membership_wire::MemberWire"
)]
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
    /// Absolute file offset of the four-byte object-id word.
    pub source_offset: u64,
}

impl RmFastLoadObjectIdTable {
    pub fn raw_count(&self) -> [u8; 4] {
        self.members.count().to_le_bytes()
    }
}
impl RmFastLoadObjectId {
    pub fn raw(&self) -> [u8; 4] {
        self.value.to_le_bytes()
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBlockControlFormKind {
    ZeroPrefixed {
        value_count: std::num::NonZeroU32,
    },
    ProductAnchored {
        leading: Option<ControlLeadingValue>,
        value_count: std::num::NonZeroU32,
        byte_len: std::num::NonZeroU64,
    },
}

/// Atomic classification of one complete offset-store control lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DataBlockControlFormWire",
    into = "DataBlockControlFormWire"
)]
pub struct DataBlockControlForm {
    /// Globally unique control-form identity.
    pub id: String,
    /// Opening control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Selected complete control grammar.
    pub kind: DataBlockControlFormKind,
    /// Absolute file offset of the control block.
    pub source_offset: u64,
}

impl DataBlockControlFormKind {
    pub fn value_count(self) -> u32 {
        match self {
            Self::ZeroPrefixed { value_count } | Self::ProductAnchored { value_count, .. } => {
                value_count.get()
            }
        }
    }

    pub fn byte_len(self) -> u64 {
        match self {
            Self::ZeroPrefixed { value_count } => u64::from(value_count.get()) * 4,
            Self::ProductAnchored { byte_len, .. } => byte_len.get(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DataBlockControlFormWire {
    id: String,
    data_block: String,
    kind: DataBlockControlFormKindWire,
    value_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    leading_value_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    leading_value: Option<u32>,
    byte_len: u64,
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DataBlockControlFormKindWire {
    ZeroPrefixed,
    ProductAnchored,
}

impl From<DataBlockControlForm> for DataBlockControlFormWire {
    fn from(value: DataBlockControlForm) -> Self {
        let (kind, leading_value_width, leading_value) = match value.kind {
            DataBlockControlFormKind::ZeroPrefixed { .. } => {
                (DataBlockControlFormKindWire::ZeroPrefixed, None, None)
            }
            DataBlockControlFormKind::ProductAnchored { leading, .. } => (
                DataBlockControlFormKindWire::ProductAnchored,
                leading.map(ControlLeadingValue::width),
                leading.map(ControlLeadingValue::value),
            ),
        };
        Self {
            id: value.id,
            data_block: value.data_block,
            kind,
            value_count: value.kind.value_count(),
            leading_value_width,
            leading_value,
            byte_len: value.kind.byte_len(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DataBlockControlFormWire> for DataBlockControlForm {
    type Error = String;

    fn try_from(wire: DataBlockControlFormWire) -> Result<Self, Self::Error> {
        let value_count = std::num::NonZeroU32::new(wire.value_count)
            .ok_or("control-form value_count must be nonzero")?;
        let byte_len = std::num::NonZeroU64::new(wire.byte_len)
            .ok_or("control-form byte_len must be nonzero")?;
        let kind = match (wire.kind, wire.leading_value_width, wire.leading_value) {
            (DataBlockControlFormKindWire::ZeroPrefixed, None, None) => {
                let kind = DataBlockControlFormKind::ZeroPrefixed { value_count };
                if kind.byte_len() != byte_len.get() {
                    return Err(
                        "control-form byte_len must equal four times value_count".to_owned()
                    );
                }
                kind
            }
            (DataBlockControlFormKindWire::ProductAnchored, None, None) => {
                DataBlockControlFormKind::ProductAnchored {
                    leading: None,
                    value_count,
                    byte_len,
                }
            }
            (DataBlockControlFormKindWire::ProductAnchored, Some(width), Some(value)) => {
                DataBlockControlFormKind::ProductAnchored {
                    leading: Some(ControlLeadingValue::from_wire(width, value)?),
                    value_count,
                    byte_len,
                }
            }
            _ => {
                return Err(
                    "control-form leading value is present only for product-anchored forms"
                        .to_owned(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            data_block: wire.data_block,
            kind,
            source_offset: wire.source_offset,
        })
    }
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
    pub value: crate::om::control_word::ControlWord24,
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
#[serde(
    try_from = "DataBlockControlClassReferenceWire",
    into = "DataBlockControlClassReferenceWire"
)]
pub struct DataBlockControlClassReference {
    /// Globally unique class-reference identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based order in the class-selection lane.
    pub ordinal: u32,
    /// Zero-based ordinal in the store's class registry.
    pub class_ordinal: u32,
    /// Retained class definition and name when that registry slot exists.
    pub class: Option<DataBlockControlClassRef>,
    /// Absolute file offset of the four-byte control word.
    pub source_offset: u64,
}

/// Retained class-definition identity and registered name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBlockControlClassRef {
    pub definition: String,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
struct DataBlockControlClassReferenceWire {
    id: String,
    data_block: String,
    ordinal: u32,
    class_ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class_definition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
    source_offset: u64,
}

impl From<DataBlockControlClassReference> for DataBlockControlClassReferenceWire {
    fn from(value: DataBlockControlClassReference) -> Self {
        let (class_definition, class_name) = match value.class {
            Some(class) => (Some(class.definition), Some(class.name)),
            None => (None, None),
        };
        Self {
            id: value.id,
            data_block: value.data_block,
            ordinal: value.ordinal,
            class_ordinal: value.class_ordinal,
            class_definition,
            class_name,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DataBlockControlClassReferenceWire> for DataBlockControlClassReference {
    type Error = String;

    fn try_from(wire: DataBlockControlClassReferenceWire) -> Result<Self, Self::Error> {
        let class = match (wire.class_definition, wire.class_name) {
            (None, None) => None,
            (Some(definition), Some(name)) => Some(DataBlockControlClassRef { definition, name }),
            _ => {
                return Err(
                    "control class reference definition and name are present together".to_owned(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            data_block: wire.data_block,
            ordinal: wire.ordinal,
            class_ordinal: wire.class_ordinal,
            class,
            source_offset: wire.source_offset,
        })
    }
}

/// Ordered object reference carried by an offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DataBlockReferenceWire", into = "DataBlockReferenceWire")]
pub struct DataBlockReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based reference order within the block.
    pub ordinal: u32,
    /// Referenced persistent OM object ID.
    pub object: crate::om::reference_index::FeatureReferenceToken,
    /// Uniquely resolved object record in the same directory entry.
    pub target_record: Option<String>,
    /// Uniquely resolved parameter declaration carrying this object ID.
    pub target_expression_declaration: Option<String>,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DataBlockReferenceWire {
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

impl From<DataBlockReference> for DataBlockReferenceWire {
    fn from(value: DataBlockReference) -> Self {
        Self {
            id: value.id,
            data_block: value.data_block,
            ordinal: value.ordinal,
            object_id: value.object.value(),
            raw_object_id: value.object.raw().to_vec(),
            target_record: value.target_record,
            target_expression_declaration: value.target_expression_declaration,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DataBlockReferenceWire> for DataBlockReference {
    type Error = String;
    fn try_from(value: DataBlockReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            data_block: value.data_block,
            ordinal: value.ordinal,
            object: crate::om::reference_index::FeatureReferenceToken::from_wire(
                value.object_id,
                &value.raw_object_id,
            )
            .map_err(|error| format!("object_id/raw_object_id: {error}"))?,
            target_record: value.target_record,
            target_expression_declaration: value.target_expression_declaration,
            source_offset: value.source_offset,
        })
    }
}

/// Complete named NX part palette for color indices 1 through 216.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "color_wire::PartColorTableWire",
    into = "color_wire::PartColorTableWire"
)]
pub struct PartColorTable {
    /// Globally unique table identity.
    pub id: String,
    /// Registered `UGS::COLOR_table` declaration in `class_definitions`.
    pub class_definition: String,
    /// Exact background components and their absolute file offsets.
    pub background: [(ColorComponent, u64); 3],
    /// Ordered entries in the native `part_color_definitions` arena.
    pub definitions: [String; PALETTE_SIZE],
    /// Directory entry containing the table.
    pub source_entry: String,
    /// Absolute file offset of the counted name roster.
    pub source_offset: u64,
}

/// One named RGB entry from an NX part palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "color_wire::PartColorDefinitionWire",
    into = "color_wire::PartColorDefinitionWire"
)]
pub struct PartColorDefinition {
    /// Globally unique color-definition identity.
    pub id: String,
    /// Owning table in the native `part_color_tables` arena.
    pub color_table: String,
    /// One-based NX color index.
    pub color_index: PaletteIndex,
    /// Serialized color name.
    pub name: String,
    /// Exact normalized components and their absolute file offsets.
    pub components: [(ColorComponent, u64); 3],
    /// Absolute file offset of the opening `05` marker.
    pub source_offset: u64,
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
    /// Consecutive target and linked rows with their checked index interval.
    #[serde(flatten)]
    pub rows: ColumnIndexRows,
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
    pub version: crate::om::product::ProductText<String>,
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
    /// Persistent OM object identifier.
    pub object_id: u32,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: PrintableString<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `66 32 03` marker.
    pub source_offset: u64,
}

#[cfg(test)]
mod printable_value_wire_tests {
    use super::StringValue;

    #[test]
    fn object_record_requires_identity_and_its_offset() {
        let wire = serde_json::json!({
            "id": "record", "object_id": 1, "object_id_source_offset": 10,
            "section_ordinal": 0, "record_ordinal": 0, "section_offset": 0,
            "byte_len": 1, "sha256": "hash", "source_entry": "entry", "source_offset": 20
        });
        let record: super::ObjectRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        for field in ["object_id", "object_id_source_offset"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::Value::Null;
            assert!(serde_json::from_value::<super::ObjectRecord>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let mut invalid = wire;
        invalid["object_id"] = serde_json::Value::Null;
        invalid["object_id_source_offset"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<super::ObjectRecord>(invalid).is_err());
    }

    #[test]
    fn retained_printable_value_preserves_spaces_and_rejects_invalid_text() {
        let json = r#"{"id":"value","record":"record","object_id":1,"ordinal":0,"value":"  A ~ ","source_entry":"entry","source_offset":10}"#;
        let value: StringValue = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        for invalid in ["", "\n", "μ"] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire["value"] = invalid.into();
            assert!(serde_json::from_value::<StringValue>(wire)
                .unwrap_err()
                .to_string()
                .contains("value"));
        }
    }
}

/// Ordered tagged-reference occurrence owned by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier.
    pub object_id: u32,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Typed reference and its same-section target when present.
    #[serde(flatten, with = "reference_wire")]
    pub reference: RecordReference<String>,
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
    /// Persistent OM object identifier.
    pub object_id: u32,
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
    /// Self-identifying reference payload.
    #[serde(flatten)]
    pub reference: DirectReference,
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
    /// Ordered encoded handle tokens with derived closing and length fields.
    #[serde(flatten)]
    pub handles: ExtrefHandles,
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
    pub tagged_reference: crate::om::reference_value::Tagged28,
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
    pub slot: ExtrefSlot,
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
        .map(|asset| (asset.storage_path(), asset))
        .collect::<BTreeMap<_, _>>();
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
            let Some(source_offset) = record
                .source_offset
                .checked_add(record.handles.prefix_byte_len() as u64)
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
            if record
                .source_offset
                .checked_add(ExtrefSlot::Fourth.offset())
                .is_none()
            {
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
            ExtrefSlot::ALL
                .into_iter()
                .zip(resolved)
                .map(|(slot, reference)| {
                    let record_key = record
                        .id
                        .split_once('#')
                        .map_or(record.id.as_str(), |(_, key)| key);
                    ExternalReferenceRecordStringUse {
                        id: format!(
                            "nx:external-reference:record-string-use#{record_key}-{}",
                            u8::from(slot)
                        ),
                        external_record: record.id.clone(),
                        slot,
                        string_index: record.id_slots[slot.index()],
                        external_reference: reference.id.clone(),
                        source_offset: record.source_offset + slot.offset(),
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
            if [slot0.slot, slot1.slot, slot2.slot, slot3.slot] != ExtrefSlot::ALL {
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
        let entry_index = entry.index();
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.types.iter().cloned().enumerate() {
            definitions.insert(
                (entry_index, definition.offset),
                ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.registry_tail[0],
                    registry_suffix: definition.registry_tail[1..].to_vec(),
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = entry.index();
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.types.iter().cloned().enumerate() {
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.registry_tail[0],
                    registry_suffix: definition.registry_tail[1..].to_vec(),
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

struct RegistryLayout<'a> {
    prefix: &'a [u8],
    fingerprint: [u8; 8],
    terminal: u8,
}

fn registry_layout(suffix: &[u8]) -> Option<RegistryLayout<'_>> {
    if !(11..=14).contains(&suffix.len()) {
        return None;
    }
    let fingerprint_start = suffix.len() - 9;
    Some(RegistryLayout {
        prefix: &suffix[..fingerprint_start],
        fingerprint: suffix[fingerprint_start..fingerprint_start + 8]
            .try_into()
            .ok()?,
        terminal: suffix[fingerprint_start + 8],
    })
}

/// Decode member definitions from every framed OM section.
pub fn field_definitions(container: &Container) -> Vec<FieldDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = entry.index();
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.fields.iter().cloned().enumerate() {
            definitions.insert(
                (entry_index, definition.offset),
                FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.registry_tail[0],
                    registry_suffix: definition.registry_tail[1..].to_vec(),
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = entry.index();
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.fields.iter().cloned().enumerate() {
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.registry_tail[0],
                    registry_suffix: definition.registry_tail[1..].to_vec(),
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
            let RecordReference::RecordOrdinal16 { ordinal, .. } = reference.value else {
                continue;
            };
            let target = usize::from(ordinal);
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
                    object_id: (record.object_id.0, entry_offset + record.object_id.1),
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
        .into_vec()
        .into_iter()
        .enumerate()
        .map(|(ordinal, object_id)| RmFastLoadObjectId {
            id: format!("nx:rmfastload:object-id#{ordinal:010}"),
            table: table_id.clone(),
            ordinal: ordinal as u32,
            value: object_id.value,
            stable_identity: None,
            source_offset: entry_offset + object_id.offset as u64,
        })
        .collect::<Vec<_>>();
    assign_rmfastload_object_id_identities(&mut object_ids);
    let native_table = RmFastLoadObjectIdTable {
        id: table_id,
        members: ObjectIdMembers::new(
            object_ids
                .iter()
                .map(|object_id| object_id.id.clone())
                .collect(),
        )
        .ok()?,
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
            let kind = match crate::om::offset_store_control_form(
                control.bytes,
                records.first().map(|record| record.bytes),
            )? {
                crate::om::OffsetStoreControlForm::ZeroPrefixed { values } => {
                    DataBlockControlFormKind::ZeroPrefixed {
                        value_count: std::num::NonZeroU32::new(u32::try_from(values.len()).ok()?)?,
                    }
                }
                crate::om::OffsetStoreControlForm::ProductAnchored {
                    leading_value,
                    values,
                } => DataBlockControlFormKind::ProductAnchored {
                    leading: leading_value,
                    value_count: std::num::NonZeroU32::new(u32::try_from(values.len()).ok()?)?,
                    byte_len: std::num::NonZeroU64::new(control.bytes.len() as u64)?,
                },
            };
            Some(DataBlockControlForm {
                id: format!("nx:om-data-block-control-forms:form#{section_ordinal}"),
                data_block: format!("nx:om-data-blocks-{section_ordinal}:block#0"),
                kind,
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
                .filter(|(candidate, _)| candidate.index() == entry.index())
                .flat_map(|(_, section)| section.types.iter().cloned().collect::<Vec<_>>())
                .chain(
                    container
                        .indexed_om_sections()
                        .into_iter()
                        .filter(|(candidate, _)| candidate.index() == entry.index())
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
            let entry_index = entry.index();
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
                        class: definition.map(|definition| DataBlockControlClassRef {
                            definition: format!(
                                "nx:om-entry-{entry_index}:class#{}",
                                definition.offset
                            ),
                            name: definition.name.to_string(),
                        }),
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
            let leading_value_width = leading_value.map_or(0, ControlLeadingValue::width);
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
                .enumerate()
                .map(|(ordinal, reference)| DataBlockControlReference {
                    id: format!(
                        "nx:om-data-block-control-references-{section_ordinal}:reference#{}",
                        reference.offset
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    reference: reference.value,
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
    let mut by_block = BTreeMap::<&str, Vec<(&DataBlockControlReference, u32)>>::new();
    for reference in references {
        let DirectReference::PersistentHandle(handle) = reference.reference else {
            continue;
        };
        by_block
            .entry(reference.data_block.as_str())
            .or_default()
            .push((reference, handle));
    }
    let mut pairs = Vec::new();
    for (data_block, mut block_references) in by_block {
        block_references.sort_by_key(|(reference, _)| reference.source_offset);
        let mut at = 0;
        while at < block_references.len() {
            let start = at;
            while block_references.get(at + 1).is_some_and(|next| {
                next.0.source_offset == block_references[at].0.source_offset + 5
            }) {
                at += 1;
            }
            let run = &block_references[start..=at];
            if let [(first, first_handle), (second, second_handle)] = run {
                pairs.push(DataBlockControlHandlePair {
                    id: format!(
                        "nx:om-data-block-control:handle-pair#{}",
                        first.source_offset
                    ),
                    data_block: data_block.to_string(),
                    first_reference: first.id.clone(),
                    second_reference: second.id.clone(),
                    first_handle: *first_handle,
                    second_handle: *second_handle,
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
        let (object_id, _) = record.object_id;
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
                            let key = (entry.name.clone(), reference.object_index.value());
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
                                object: reference.object_index,
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
        let entry_index = entry.index();
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let source_base = entry_offset + storage_offset as u64;
        let table_id = format!("nx:part-color-tables:table#{section_ordinal}");
        let parsed_definitions = PaletteIndex::all().map(|color_index| {
            let definition = &table.definitions[usize::from(color_index.value()) - 1];
            PartColorDefinition {
                id: format!(
                    "nx:part-color-definitions-{section_ordinal}:color#{}",
                    color_index.value()
                ),
                color_table: table_id.clone(),
                color_index,
                name: definition.name.to_string(),
                components: definition
                    .components
                    .map(|(component, offset)| (component, source_base + offset as u64)),
                source_offset: source_base + definition.offset as u64,
            }
        });
        let definition_ids = parsed_definitions
            .each_ref()
            .map(|definition| definition.id.clone());
        definitions.extend(parsed_definitions);
        tables.push(PartColorTable {
            id: table_id,
            class_definition: format!("nx:om-entry-{entry_index}:class#{}", class.offset),
            background: table
                .background
                .map(|(component, offset)| (component, source_base + offset as u64)),
            definitions: definition_ids,
            source_entry: entry.name.clone(),
            source_offset: source_base + table.offset as u64,
        });
    }

    (tables, definitions)
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
            if opening.frame.mode() != crate::om::discriminators::IndexRowMode::Form07
                || suffix.is_empty()
                || suffix
                    .iter()
                    .any(|row| row.frame.mode() != crate::om::discriminators::IndexRowMode::Form04)
                || last_target.frame.mode() != crate::om::discriminators::IndexRowMode::Form04
                || target_prefix
                    .iter()
                    .any(|row| row.frame.mode() != crate::om::discriminators::IndexRowMode::Form07)
            {
                return None;
            }
            let ordered = std::iter::once((
                opening.frame.target_index().atom.value(),
                opening.frame.offset(),
            ))
            .chain(
                targets
                    .iter()
                    .map(|row| (row.frame.target_index().atom.value(), row.frame.offset())),
            )
            .chain(
                suffix
                    .iter()
                    .map(|row| (row.frame.target_index().atom.value(), row.frame.offset())),
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
                rows: ColumnIndexRows::new(
                    opening.frame.target_index().atom.value(),
                    targets.iter().map(|row| row.id.clone()).collect(),
                    suffix.iter().map(|row| row.id.clone()).collect(),
                )
                .ok()?,
                source_entry: opening.source_entry.clone(),
                source_offset: opening.frame.offset(),
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
                            version: version.value.into_owned(),
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
                                version: version.value.into_owned(),
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
                .filter_map(move |(record_ordinal, value_ordinal, object_id, value)| {
                    let object_id = object_id?;
                    let record =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    Some(StringValue {
                        id: format!(
                            "nx:om-string-values-{section_ordinal}-{record_ordinal}:value#{}",
                            value.offset
                        ),
                        record,
                        object_id,
                        ordinal: value_ordinal as u32,
                        value: value.value.into_owned(),
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
                .filter_map(
                    move |(record_ordinal, reference_ordinal, object_id, reference)| {
                        let object_id = object_id?;
                        let record = format!(
                            "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                        );
                        Some(ObjectReference {
                            id: format!(
                                "nx:om-references-{section_ordinal}-{record_ordinal}:reference#{}",
                                reference.offset
                            ),
                            record,
                            object_id,
                            ordinal: reference_ordinal as u32,
                            reference: match reference.value {
                                RecordReference::Direct(value) => RecordReference::Direct(value),
                                RecordReference::RecordOrdinal16 { ordinal, .. } => RecordReference::RecordOrdinal16 {
                                    ordinal,
                                    target: format!("nx:om-record-directory-{section_ordinal}:entry#{ordinal}"),
                                },
                            },
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + reference.offset as u64,
                        })
                    },
                )
                .collect()
        })
        .collect()
}

/// Join maximal two-token adjacent persistent-handle runs within object records.
pub fn object_record_handle_pairs(references: &[ObjectReference]) -> Vec<ObjectRecordHandlePair> {
    let mut by_record = BTreeMap::<&str, Vec<(&ObjectReference, u32)>>::new();
    for reference in references {
        let RecordReference::Direct(DirectReference::PersistentHandle(handle)) =
            reference.reference
        else {
            continue;
        };
        by_record
            .entry(reference.record.as_str())
            .or_default()
            .push((reference, handle));
    }
    let mut pairs = Vec::new();
    for (record, mut record_references) in by_record {
        record_references.sort_by_key(|(reference, _)| reference.source_offset);
        let mut at = 0;
        while at < record_references.len() {
            let start = at;
            while record_references.get(at + 1).is_some_and(|next| {
                next.0.source_offset == record_references[at].0.source_offset + 5
            }) {
                at += 1;
            }
            let run = &record_references[start..=at];
            if let [(first, first_handle), (second, second_handle)] = run {
                pairs.push(ObjectRecordHandlePair {
                    id: format!("nx:om-object-record:handle-pair#{}", first.source_offset),
                    record: record.to_string(),
                    object_id: first.object_id,
                    first_reference: first.id.clone(),
                    second_reference: second.id.clone(),
                    first_handle: *first_handle,
                    second_handle: *second_handle,
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
    for reference in references {
        let RecordReference::Direct(DirectReference::PersistentHandle(handle)) =
            reference.reference
        else {
            continue;
        };
        let group = groups.entry(handle).or_default();
        group.occurrence_count += 1;
        if group.records.last() != Some(&reference.record)
            && !group.records.contains(&reference.record)
        {
            group.records.push(reference.record.clone());
        }
    }
    for reference in control_references {
        let DirectReference::PersistentHandle(handle) = reference.reference else {
            continue;
        };
        let group = groups.entry(handle).or_default();
        group.occurrence_count += 1;
        if !group.data_blocks.contains(&reference.data_block) {
            group.data_blocks.push(reference.data_block.clone());
        }
    }
    for record in external {
        for handle in record.handles.serialized() {
            let group = groups.entry(*handle).or_default();
            group.external_occurrence_count += 1;
            if !group.external_records.contains(&record.id) {
                group.external_records.push(record.id.clone());
            }
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
                        name: declaration.name.into_owned(),
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
                    object_id,
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
                .get(&(entry.name.as_str(), expression.name.as_str()))
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
            let value = expression.constant_value();
            let Some(source_table) = cadmpeg_ir::NonEmptyString::new(format!(
                "nx:om-entry-{entry_index}:expression-table#{table_offset}"
            )) else {
                continue;
            };
            expressions.push(Expression {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                owner: indexed_record
                    .map(|(object_id, record)| ExpressionOwner { object_id, record }),
                declaration,
                name: expression.name.into_owned(),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Inch => ExpressionUnit::Inch,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                    crate::om::ExpressionUnit::Native(unit) => ExpressionUnit::Native(unit),
                },
                expression: expression.expression.to_string(),
                value,
                source_entry: entry.name.clone(),
                source_table,
                source_offset: entry_offset + expression.offset as u64,
            });
        }
    }
    evaluate_expression_graphs(&mut expressions);
    expressions
}

pub(crate) fn evaluate_expression_graphs(expressions: &mut [Expression]) {
    let mut name_counts = BTreeMap::<(String, String, ExpressionUnit), usize>::new();
    for expression in expressions.iter() {
        *name_counts
            .entry((
                expression.source_table.as_str().to_string(),
                expression.name.as_str().to_string(),
                expression.unit.clone(),
            ))
            .or_default() += 1;
    }
    let mut values = BTreeMap::<(String, String, ExpressionUnit), f64>::new();
    for expression in expressions.iter_mut() {
        let key = (
            expression.source_table.as_str().to_string(),
            expression.name.as_str().to_string(),
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
                expression.source_table.as_str().to_string(),
                expression.name.as_str().to_string(),
                expression.unit.clone(),
            );
            if name_counts.get(&expression_key) != Some(&1) {
                continue;
            }
            let evaluated = evaluate_parameterized_expression(&expression.expression, |name| {
                let key = (
                    expression.source_table.as_str().to_string(),
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
    #[test]
    fn data_block_reference_wire_preserves_feature_token_and_rejects_mismatch() {
        for (value, raw) in [
            (0, vec![0]),
            (0, vec![0x80, 0]),
            (0, vec![0x90, 0, 0]),
            (6466, vec![0x90, 0x19, 0x42]),
        ] {
            let wire = serde_json::json!({"id":"reference", "data_block":"block", "ordinal":0,
                "object_id":value, "raw_object_id":raw, "source_offset":12});
            let record: super::DataBlockReference = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(record).unwrap(), wire);
        }
        for (value, raw) in [
            (1, vec![0]),
            (0, vec![0xff]),
            (0, vec![0xf0, 0]),
            (0, vec![0x90, 0]),
            (0, vec![0, 0]),
        ] {
            let wire = serde_json::json!({"id":"reference", "data_block":"block", "ordinal":0,
                "object_id":value, "raw_object_id":raw, "source_offset":12});
            assert!(serde_json::from_value::<super::DataBlockReference>(wire)
                .unwrap_err()
                .to_string()
                .contains("object_id/raw_object_id"));
        }
    }

    mod expression_wire;
    mod native_units;
    mod state_counters;
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};

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
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
                owner: None,
                declaration: None,
                name: crate::om::parameter_name::ParameterName::new(name.to_string()),
                unit: super::ExpressionUnit::Millimeter,
                expression: formula.into(),
                value,
                source_entry: "part".into(),
                source_table: cadmpeg_ir::NonEmptyString::new(table).unwrap(),
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
                owner: None,
                declaration: None,
                name: crate::om::parameter_name::ParameterName::new(name.to_string()),
                unit: super::ExpressionUnit::Millimeter,
                expression: formula.into(),
                value,
                source_entry: "part".into(),
                source_table: cadmpeg_ir::NonEmptyString::new(table).unwrap(),
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
                    owner: None,
                    declaration: None,
                    name: crate::om::parameter_name::ParameterName::new(name.to_string()),
                    unit,
                    expression: formula.into(),
                    value,
                    source_entry: "part".into(),
                    source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
        let expression = |key: u32, name: &str, text: &str, value: Option<f64>| super::Expression {
            id: format!("nx:test:expression#{key}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: text.into(),
            value,
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
            source_offset: u64::from(key),
        };
        let expressions = [
            expression(20, "p2", "5", Some(5.0)),
            expression(21, "p2_radius", "7", Some(7.0)),
            expression(90, "p9", "p2_radius * 2 + p2_radius", None),
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
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: text.into(),
            value: None,
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
                owner: None,
                declaration: None,
                name: crate::om::parameter_name::ParameterName::new(name.to_string()),
                unit,
                expression: text.into(),
                value,
                source_entry: "/Root/UG_PART/UG_PART".into(),
                source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
        assert!(feature_completeness::incomplete_expression_parameters(&ir).is_empty());

        ir.model.parameters[0]
            .properties
            .insert("unit".into(), "native".into());
        assert_eq!(
            feature_completeness::incomplete_expression_parameters(&ir),
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
                owner: None,
                declaration: None,
                name: crate::om::parameter_name::ParameterName::new(name.to_string()),
                unit: super::ExpressionUnit::Millimeter,
                expression: text.into(),
                value: None,
                source_entry: "/Root/UG_PART/UG_PART".into(),
                source_table: cadmpeg_ir::NonEmptyString::new(table).unwrap(),
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
        assert!(feature_completeness::incomplete_expression_parameters(&ir).is_empty());

        let mut inconsistent = ir.clone();
        inconsistent.model.parameters[1].value = Some(
            cadmpeg_ir::features::ParameterValue::Length(cadmpeg_ir::features::Length(1.0)),
        );
        assert_eq!(
            feature_completeness::incomplete_expression_parameters(&inconsistent),
            [inconsistent.model.parameters[1].id.clone()].into()
        );

        let mut duplicate_name = ir.clone();
        duplicate_name.model.parameters[1].name = duplicate_name.model.parameters[0].name.clone();
        assert_eq!(
            feature_completeness::incomplete_expression_parameters(&duplicate_name),
            duplicate_name.model.parameters[..2]
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect()
        );

        let mut unevaluated = ir.clone();
        unevaluated.model.parameters[1].value = None;
        assert_eq!(
            feature_completeness::incomplete_expression_parameters(&unevaluated),
            [unevaluated.model.parameters[1].id.clone()].into()
        );

        let mut operation_owned = unevaluated;
        operation_owned.model.features[0].definition =
            cadmpeg_ir::features::FeatureDefinition::Native {
                kind: "TEST_OPERATION".into(),
                parameters: BTreeMap::default(),
            };
        assert_eq!(
            feature_completeness::incomplete_expression_parameters(&operation_owned),
            [operation_owned.model.parameters[1].id.clone()].into()
        );
    }

    #[test]
    fn nx_cyclic_formula_table_omits_invalid_neutral_dependency_edges() {
        let expression = |id: &str, name: &str, text: &str, source_offset| super::Expression {
            id: format!("nx:test:expression#{id}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: text.to_string(),
            value: None,
            source_entry: "part".to_string(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
            feature_completeness::incomplete_expression_parameters(&ir),
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect()
        );
        let mut losses = Vec::new();
        crate::decode::report::append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("2 NX expression parameter(s)"));
    }

    #[test]
    fn nx_cyclic_formula_table_retains_independent_acyclic_dependencies() {
        let expression = |id: &str, name: &str, text: &str, source_offset| super::Expression {
            id: format!("nx:test:expression#{id}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: text.to_string(),
            value: None,
            source_entry: "part".to_string(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
            feature_completeness::incomplete_expression_parameters(&ir),
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
            input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
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
        assert_eq!(
            uses[0]
                .bindings
                .iter()
                .map(|binding| binding.binding.as_str())
                .collect::<Vec<_>>(),
            ["early", "late"]
        );
        assert_eq!(
            uses[0]
                .bindings
                .iter()
                .map(|binding| binding.source_offset)
                .collect::<Vec<_>>(),
            [20, 30]
        );

        let expression = super::Expression {
            id: "nx:test:expression#20".to_string(),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new("p20".to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: "5".to_string(),
            value: Some(5.0),
            source_entry: "part".to_string(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new("p20".to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: "5".to_string(),
            value: Some(5.0),
            source_entry: "part".to_string(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
            source_offset: 10,
        };
        let parameter_use = |id: &str, operation: &str, source_offset| {
            crate::native::features::FeatureParameterUse {
                id: id.to_string(),
                operation_label: operation.to_string(),
                expression: expression.id.clone(),
                bindings: vec![crate::native::features::FeatureParameterUseBinding {
                    binding: format!("binding-{id}"),
                    source_offset,
                }],
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
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new("p20".to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: "5".to_string(),
            value: Some(5.0),
            source_entry: "part".to_string(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
            source_offset: 20,
        };
        let parameter_use = crate::native::features::FeatureParameterUse {
            id: "use".to_string(),
            operation_label: "nx:feature-history:operation-label#1-2".to_string(),
            expression: expression.id.clone(),
            bindings: vec![crate::native::features::FeatureParameterUseBinding {
                binding: "binding".to_string(),
                source_offset: 30,
            }],
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
                cadmpeg_ir::features::ParameterId::mint("nx:test:parameter#20")
                    .expect("identity grammar"),
                cadmpeg_ir::features::ParameterId::mint("nx:test:parameter#20")
                    .expect("identity grammar"),
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
            input_slot: crate::om::header_references::HeaderSlot::Zero,
            object: crate::om::reference_index::FeatureReferenceToken::from_wire(45, &[45])
                .unwrap(),
            data_block: "nx:om-data-blocks-2:block#45".to_string(),
            source_offset: 700,
        };
        let reference = |ordinal: u32, declaration: Option<&str>| DataBlockReference {
            id: format!("nx:om-data-block-references-2-45:reference#{ordinal}"),
            data_block: input.data_block.clone(),
            ordinal,
            object: crate::om::reference_index::FeatureReferenceToken::from_wire(
                201 + ordinal,
                &[0x80, (201 + ordinal) as u8],
            )
            .unwrap(),
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
            owner: None,
            declaration: Some("nx:om-expression-declarations-0:declaration#3".to_string()),
            name: crate::om::parameter_name::ParameterName::new("p3".to_string()),
            unit: super::ExpressionUnit::Millimeter,
            expression: "12".to_string(),
            value: Some(12.0),
            source_entry: "/Root/UG_PART/UG_PART".to_string(),
            source_table: cadmpeg_ir::NonEmptyString::new("table").unwrap(),
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
        assert_eq!(bindings[0].input_slot.number(), 0);
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
                leading_value: Some(
                    crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
                ),
                values: crate::om::nonempty::NonEmpty::new([7, 0x1020]).unwrap(),
            })
        );

        let mut nonzero_leading = vec![0x34, 0x12, 0x00];
        nonzero_leading.extend_from_slice(&7u32.to_le_bytes());
        nonzero_leading.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
        assert_eq!(
            crate::om::offset_store_control_form(&nonzero_leading, None),
            Some(crate::om::OffsetStoreControlForm::ProductAnchored {
                leading_value: Some(
                    crate::om::control_leading_value::ControlLeadingValue::from_wire(3, 0x1234)
                        .unwrap()
                ),
                values: crate::om::nonempty::NonEmpty::new([7]).unwrap(),
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
        assert_eq!(
            forms[0].kind,
            super::DataBlockControlFormKind::ZeroPrefixed {
                value_count: std::num::NonZeroU32::new(2).unwrap()
            }
        );
        assert_eq!(forms[0].kind.value_count(), 2);
        assert_eq!(forms[0].kind.byte_len(), blocks[0].byte_len);
        let control_values = super::data_block_control_values(&container);
        assert_eq!(control_values.len(), 2);
        assert_eq!(control_values[0].data_block, blocks[0].id);
        assert_eq!(control_values[0].ordinal, 0);
        assert_eq!(control_values[0].value.value(), 0);
        assert_eq!(control_values[1].value.value(), 1);
        let classes = super::data_block_control_class_references(&container);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].data_block, blocks[0].id);
        assert_eq!(classes[0].ordinal, 0);
        assert_eq!(classes[0].class_ordinal, 0);
        assert_eq!(
            classes[0].class.as_ref().map(|class| class.name.as_str()),
            Some("UGS::ModlFeature")
        );
        assert_eq!(
            classes[0]
                .class
                .as_ref()
                .map(|class| class.definition.as_str()),
            Some("nx:om-entry-0:class#8")
        );
        assert!(super::string_values(&container).is_empty());
        assert!(super::object_references(&container).is_empty());
        let expressions = super::expressions(&container);
        assert_eq!(expressions.len(), 1);
        assert_eq!(
            expressions[0].owner.as_ref().map(|owner| owner.object_id),
            None
        );
        assert_eq!(
            expressions[0].owner.as_ref().map(|owner| &owner.record),
            None
        );
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
    fn control_form_wire_checks_nonempty_counts_and_derived_length() {
        let json = r#"{"id":"c","data_block":"b","kind":"zero_prefixed","value_count":2,"byte_len":8,"source_offset":0}"#;
        let value: super::DataBlockControlForm = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        for (field, invalid) in [("value_count", 0), ("byte_len", 0), ("byte_len", 7)] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = invalid.into();
            assert!(serde_json::from_value::<super::DataBlockControlForm>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let json = r#"{"id":"c","data_block":"b","kind":"product_anchored","value_count":2,"byte_len":1,"source_offset":0}"#;
        let value: super::DataBlockControlForm = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        for field in ["value_count", "byte_len"] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = 0.into();
            assert!(serde_json::from_value::<super::DataBlockControlForm>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }

    #[test]
    fn control_leading_value_preserves_wire_and_rejects_width_mismatch() {
        let json = r#"{"id":"c","data_block":"b","kind":"product_anchored","value_count":2,"leading_value_width":2,"leading_value":0,"byte_len":26,"source_offset":0}"#;
        let value: super::DataBlockControlForm = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["leading_value_width"] = 4.into();
        assert!(
            serde_json::from_value::<super::DataBlockControlForm>(wire.clone())
                .unwrap_err()
                .to_string()
                .contains("leading_value_width")
        );
        wire["leading_value_width"] = 2.into();
        wire["leading_value"] = 65536.into();
        assert!(serde_json::from_value::<super::DataBlockControlForm>(wire)
            .unwrap_err()
            .to_string()
            .contains("leading_value"));
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
            super::DataBlockControlFormKind::ProductAnchored {
                leading: Some(
                    crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
                ),
                value_count: std::num::NonZeroU32::new(2).unwrap(),
                byte_len: std::num::NonZeroU64::new(26).unwrap(),
            }
        );
        assert_eq!(forms[0].kind.value_count(), 2);
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
            classes[0].class.as_ref().map(|class| class.name.as_str()),
            Some("UGS::FEATURE_RECORD")
        );
        assert!(classes[0].class.is_some());
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
        assert_eq!(expressions[0].name.as_str(), "p9");
        assert_eq!(expressions[0].expression, "p2 * 2 + p7_radius");
        assert_eq!(expressions[0].constant_value(), None);
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
        assert_eq!(expressions.len(), 1);
        assert_eq!(
            expressions[0].owner.as_ref().map(|owner| owner.object_id),
            Some(0x102)
        );
        assert_eq!(expressions[0].name.index(), Some(8));
        assert_eq!(
            expressions[0].name.qualifier(),
            Some("CircularPattern_pattern_Circular_Dir_offset_angle")
        );
        assert_eq!(
            expressions[0].name.as_str(),
            "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
        );
        assert_eq!(expressions[0].unit, super::ExpressionUnit::Degree);
        assert_eq!(expressions[0].expression, "120");
        assert_eq!(expressions[0].value, Some(120.0));
        assert_eq!(expressions[0].source_entry, "/Root/UG_PART/UG_PART");
        assert!(expressions[0]
            .source_table
            .as_str()
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
        assert_eq!(declarations[0].name.index(), 8);
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
            .find(|parameter| parameter.name == expressions[0].name.as_str())
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
        assert_eq!(headers[0].version.as_str(), "NX 2027.3102");
        assert_eq!(headers[0].object_id, Some(0x101));
        assert_eq!(object_records[1].object_id.0, 0x102);
        assert_eq!(
            object_records[1].object_id.1,
            object_records[0].object_id.1 + 4
        );
        assert_eq!(
            expressions[0].owner.as_ref().map(|owner| &owner.record),
            Some(&object_records[1].id)
        );
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
        assert_eq!(strings[0].object_id, 0x102);
        assert_eq!(strings[0].value.as_str(), "SKETCH_001");
        let references = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::ObjectReference>("object_references")
            .expect("required invariant");
        assert_eq!(references.len(), 3);
        assert_eq!(references[0].record, object_records[1].id);
        assert_eq!(references[0].object_id, 0x102);
        let wire = serde_json::to_value(&references).unwrap();
        assert_eq!(wire[0]["value"], 0x1234_5678);
        assert_eq!(wire[0]["target_record"], serde_json::Value::Null);
        assert_eq!(wire[1]["kind"], "tagged28");
        assert_eq!(wire[1]["value"], 0x0abc_def0);
        assert_eq!(wire[1]["target_record"], serde_json::Value::Null);
        assert_eq!(wire[2]["kind"], "record_ordinal16");
        assert_eq!(wire[2]["value"], 0);
        assert_eq!(
            wire[2]["target_record"].as_str(),
            Some(object_records[0].id.as_str())
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
        assert_eq!(parameter.name, expressions[0].name.as_str());
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
        let fields: Vec<_> = fields
            .iter()
            .cloned()
            .map(super::FieldDefinitionWire::from)
            .collect();
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
        let layout = super::registry_layout(&[
            0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
        ]);
        let prefix = layout
            .as_ref()
            .map_or_else(Vec::new, |layout| layout.prefix.to_vec());
        let fingerprint = layout.as_ref().map(|layout| layout.fingerprint);
        let terminal = layout.as_ref().map(|layout| layout.terminal);
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
        let classes: Vec<_> = classes
            .iter()
            .cloned()
            .map(super::ClassDefinitionWire::from)
            .collect();
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
            registry_tail: &[
                0xa0, 0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ],
        };

        let legacy = super::ClassDefinitionWire::from(super::ClassDefinition {
            id: String::new(),
            name: legacy_definition.name.into(),
            ordinal: 0,
            trailing_code: legacy_definition.registry_tail[0],
            registry_suffix: legacy_definition.registry_tail[1..].to_vec(),
            section_offset: 0,
            source_entry: String::new(),
            source_offset: 0,
        });
        assert_eq!(legacy.registry_storage_code, None);
        assert_eq!(legacy.registry_base_class, None);
        assert_eq!(legacy.registry_reference, None);
        assert_eq!(
            legacy.schema_fingerprint,
            Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
        );
        assert_eq!(legacy.layout_terminal, Some(0x06));

        let complete_definition = crate::om::TypeDefinition {
            offset: 0,
            name: "UGS::FEATURE_RECORD",
            registry_tail: &[
                0x38, 0x05, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x02,
            ],
        };
        let complete = super::ClassDefinitionWire::from(super::ClassDefinition {
            id: String::new(),
            name: complete_definition.name.into(),
            ordinal: 0,
            trailing_code: complete_definition.registry_tail[0],
            registry_suffix: complete_definition.registry_tail[1..].to_vec(),
            section_offset: 0,
            source_entry: String::new(),
            source_offset: 0,
        });
        assert_eq!(complete.registry_storage_code, Some(0x38));
        assert_eq!(complete.registry_base_class, Some(0x05));
        assert_eq!(complete.registry_reference, Some(0x02));
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
