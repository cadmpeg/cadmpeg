// SPDX-License-Identifier: Apache-2.0
//! Object-model, data-block, expression, and external-reference extractors and record types.

use cadmpeg_ir::native::bytes::NativeBytes;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::container::Container;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::om::control_leading_value::ControlLeadingValue;
use crate::om::reference_value::{DirectReference, RecordReference};
use crate::om::state_message::StateMessage;
use crate::om::state_table::StateTableEntry;
use crate::printable_string::PrintableString;
pub(super) mod journal_group;
use cadmpeg_ir::scalar::FiniteReal;
pub(super) mod material_texture;
pub(super) mod object_uuid;
mod reference_wire;
mod registry_borrowed_wires;
mod state_index_wire;
use journal_group::OmOperationStateJournalGroup;
use material_texture::MaterialTextureAsset;

pub(super) mod column_row;
pub(super) mod compact_lane;
pub(super) mod creation_display;
pub(super) mod display_color;
use column_row::{DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
mod membership_wire;
use crate::container::extref_handles::ExtrefHandles;
use crate::container::extref_slot::ExtrefSlot;
use crate::container::membership::ObjectIdMembers;
use crate::om::color::{ColorComponent, PaletteIndex, BACKGROUND_NAME, PALETTE_SIZE};
mod color_wire;
pub(super) mod column_index;
use column_index::ColumnIndexRows;

use crate::native::segments::segment_om_links;
use crate::om::parameter_name::ParameterName;
pub(super) mod roll_forward;
use crate::om::IndexedStore;
use roll_forward::OmRollForwardStateTable;
pub(super) mod state_slot_lane;
use state_slot_lane::OmOperationStateSlotLane;
pub(super) mod state_status;
use state_status::OmOperationStateStatus;

/// Semantic family declared by a linked OM section's class registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OmSchemaRole {
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
pub(super) struct OmRecordArea {
    /// Globally unique record-area identity.
    pub(super) id: String,
    /// Link identifying the owning ordered OM section.
    pub(super) section_link: String,
    /// Registry-derived role of the owning section.
    pub(super) schema_role: OmSchemaRole,
    /// Three exact little-endian control words.
    pub(super) control_words: [u32; 3],
    /// Exact printable product/version string.
    pub(super) product_version: crate::om::product::ProductText<String>,
    /// Exact record-area byte length.
    pub(super) byte_len: u64,
    /// SHA-256 of the complete pointed record area.
    pub(super) sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// Absolute file offset of the first control word.
    pub(super) source_offset: u64,
}

/// One complete row retained from an audit-trail record area.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "state_index_wire::OmAuditTrailRowWire")]
pub(super) struct OmAuditTrailRow {
    /// Globally unique audit-row identity.
    pub(super) id: String,
    /// Owning audit-trail section link.
    section_link: String,
    /// Exact framed audit content.
    record: crate::om::audit::AuditRecord,
    /// Directory entry containing the audit-trail section.
    source_entry: String,
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
        source_offset.checked_add(cadmpeg_core::decode::u64_from_index(record.byte_len()))?;
        Some(Self {
            id,
            section_link,
            record,
            source_entry,
            source_offset,
        })
    }

    fn record(&self) -> crate::om::audit::AuditRecord {
        self.record
    }
    pub(super) fn source_offset(&self) -> u64 {
        self.source_offset
    }
    fn end_offset(&self) -> u64 {
        self.source_offset + cadmpeg_core::decode::u64_from_index(self.record.byte_len())
    }
}

/// One row from the feature-history operation-state counter map.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "state_index_wire::OmOperationStateCounterWire")]
pub(super) struct OmOperationStateCounter {
    /// Globally unique counter-row identity.
    pub(super) id: String,
    /// Owning feature-history section link.
    section_link: String,
    /// Zero-based row ordinal within the section's counter map.
    ordinal: u32,
    /// Complete counter row with derived index position.
    pub(super) frame: crate::om::state_counter::StateCounter,
    /// Directory entry containing the feature-history section.
    source_entry: String,
}

/// One standalone operation-state message record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct OmOperationStateMessage {
    /// Globally unique message identity.
    pub(super) id: String,
    /// Owning feature-history section link.
    section_link: String,
    /// Zero-based message ordinal within the bounded state block.
    ordinal: u32,
    /// Diagnostic payload.
    #[serde(flatten)]
    body: StateMessage<String>,
    /// Directory entry containing the feature-history section.
    source_entry: String,
    /// Absolute file offset of the opening `03` marker.
    pub(super) source_offset: u64,
}

/// Decode internally pointed record areas from linked OM sections.
pub(super) fn om_record_areas(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmRecordArea>, cadmpeg_core::CodecError> {
    let links = segment_om_links(container)?;
    let sections = container.om_sections()?;
    let mut areas = Vec::new();
    for link in links {
        let Some((_, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(header) = section.record_area_header() else {
            continue;
        };
        let Some(area) = section.record_area else {
            continue;
        };
        let bytes = area.bytes;
        let Some(entry_offset) = link
            .location
            .section_offset()
            .checked_sub(cadmpeg_core::decode::u64_from_index(section.offset))
        else {
            continue;
        };
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(header.offset))
            .ok_or_else(|| CodecError::malformed("NX OM record area source offset overflows"))?;
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        areas.push(OmRecordArea {
            id: format!("nx:om-record-areas:area#{section_key}-{}", header.offset),
            section_link: link.id,
            schema_role: link.schema_role,
            control_words: header.control_words,
            product_version: header.product.value.into_owned(),
            byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                bytes,
                "retain source digest",
            )?,
            source_offset,
        });
    }
    Ok(areas)
}

/// Build an identity with two ten-digit minimum ordinals.
fn om_padded_state_id(prefix: &str, section_ordinal: usize, ordinal: u32) -> String {
    format!("{prefix}{section_ordinal:010}-{ordinal:010}")
}

/// Decode complete rows from audit-trail record areas.
pub(super) fn audit_trail_rows(
    container: &Container,
) -> Result<Vec<OmAuditTrailRow>, cadmpeg_core::CodecError> {
    let sections = container.om_sections()?;
    let mut out = Vec::new();
    for (section_ordinal, link) in segment_om_links(container)?
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::AuditTrail)
        .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(rows) = section.audit_trail_rows() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for row in rows {
            let record = row.record();
            let ordinal = record.ordinal.value();
            let Some(source_offset) =
                entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(row.offset()))
            else {
                continue;
            };
            if source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(record.byte_len()))
                .is_none()
            {
                continue;
            }
            let id = om_padded_state_id("nx:audit-trail:row#", section_ordinal, ordinal);
            let section_link = link.id.clone();
            let source_entry = entry.name.clone();
            let Some(result) =
                OmAuditTrailRow::new(id, section_link, record, source_entry, source_offset)
            else {
                continue;
            };

            out.push(result);
        }
    }
    Ok(out)
}

/// Decode exact object state-counter rows from canonical feature-history areas.
pub(super) fn operation_state_counters(
    container: &Container,
) -> Result<Vec<OmOperationStateCounter>, cadmpeg_core::CodecError> {
    let sections = container.om_sections()?;
    let mut out = Vec::new();
    for (section_ordinal, link) in
        crate::native::features::canonical_feature_history_links(segment_om_links(container)?)
            .into_iter()
            .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(map) = section.operation_state_counter_map() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, row) in map.into_rows().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(frame) = row.into_absolute(entry_offset) else {
                continue;
            };

            out.push(OmOperationStateCounter {
                id: om_padded_state_id(
                    "nx:feature-history:operation-state-counter#",
                    section_ordinal,
                    ordinal,
                ),
                section_link: link.id.clone(),
                ordinal,
                frame,
                source_entry: entry.name.clone(),
            });
        }
    }
    Ok(out)
}

/// Decode anchored state-journal groups from canonical feature-history areas.
pub(super) fn operation_state_journal_groups(
    container: &Container,
) -> Result<Vec<OmOperationStateJournalGroup>, cadmpeg_core::CodecError> {
    let sections = container.om_sections()?;
    let mut out = Vec::new();
    for (section_ordinal, link) in
        crate::native::features::canonical_feature_history_links(segment_om_links(container)?)
            .into_iter()
            .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(groups) = section.operation_state_journal_groups() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, group) in groups.into_iter().enumerate() {
            let Some(ordinal) = u32::try_from(ordinal).ok() else {
                continue;
            };
            let Some(frame) = group.into_absolute(entry_offset) else {
                continue;
            };

            out.push(OmOperationStateJournalGroup {
                id: om_padded_state_id(
                    "nx:feature-history:operation-state-journal-group#",
                    section_ordinal,
                    ordinal,
                ),
                section_link: link.id.clone(),
                ordinal,
                frame,
                source_entry: entry.name.clone(),
            });
        }
    }
    Ok(out)
}

/// Decode field-declared roll-forward groups from canonical feature-history areas.
pub(super) fn operation_state_groups(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmRollForwardStateTable>, CodecError> {
    let sections = container.om_sections()?;
    let mut output = Vec::new();
    for (section_ordinal, link) in
        crate::native::features::canonical_feature_history_links(segment_om_links(container)?)
            .into_iter()
            .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(table) = section.operation_state_group_table(ctx)? else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let table_end_offset =
            entry_offset + cadmpeg_core::decode::u64_from_index(table.end_offset());
        let table_footer = table.footer();
        let frames = table
            .into_groups()
            .into_iter()
            .filter_map(|group| group.into_absolute(entry_offset));
        let table = OmRollForwardStateTable::from_frames(
            section_ordinal,
            &link.id,
            &entry.name,
            table_footer,
            table_end_offset,
            frames,
        )?;

        output.push(table);
    }
    Ok(output)
}

/// Decode standalone operation-state messages from canonical feature-history areas.
pub(super) fn operation_state_messages(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmOperationStateMessage>, CodecError> {
    let sections = container.om_sections()?;
    let mut output = Vec::new();
    for (section_ordinal, link) in
        crate::native::features::canonical_feature_history_links(segment_om_links(container)?)
            .into_iter()
            .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(messages) = section.operation_state_messages(ctx)? else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, message) in messages.into_iter().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(message.offset()))
                .ok_or_else(|| CodecError::malformed("NX state message source offset overflows"))?;
            let body = message.body().into_owned()?;

            output.push(OmOperationStateMessage {
                id: om_padded_state_id(
                    "nx:feature-history:operation-state-message#",
                    section_ordinal,
                    ordinal,
                ),
                section_link: link.id.clone(),
                ordinal,
                body,
                source_entry: entry.name.clone(),
                source_offset,
            });
        }
    }
    Ok(output)
}

/// Decode exact per-object operation-state status rows from feature-history areas.
pub(super) fn operation_state_statuses(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmOperationStateStatus>, CodecError> {
    let sections = container.om_sections()?;
    let mut output = Vec::new();
    for (section_ordinal, link) in
        crate::native::features::canonical_feature_history_links(segment_om_links(container)?)
            .into_iter()
            .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(table) = section.operation_state_status_table(ctx)? else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, (offset, row)) in table
            .into_entries()
            .filter_map(|(offset, entry)| match entry {
                StateTableEntry::Status(row) => Some((offset, row)),
                StateTableEntry::Slots(_) => None,
            })
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(source_offset) =
                entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(offset))
            else {
                continue;
            };
            if source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(row.byte_len()))
                .is_none()
            {
                continue;
            }
            let body = row.into_owned()?;
            let Some(record) = OmOperationStateStatus::new(
                om_padded_state_id(
                    "nx:feature-history:operation-state-status#",
                    section_ordinal,
                    ordinal,
                ),
                link.id.clone(),
                ordinal,
                body,
                entry.name.clone(),
                source_offset,
            ) else {
                continue;
            };

            output.push(record);
        }
    }
    Ok(output)
}

/// Decode exact feature-record slot lanes from feature-history status blocks.
pub(super) fn operation_state_slot_lanes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmOperationStateSlotLane>, CodecError> {
    let sections = container.om_sections()?;
    let mut output = Vec::new();
    for (section_ordinal, link) in
        crate::native::features::canonical_feature_history_links(segment_om_links(container)?)
            .into_iter()
            .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(table) = section.operation_state_status_table(ctx)? else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, (offset, slots)) in table
            .into_entries()
            .filter_map(|(offset, entry)| match entry {
                StateTableEntry::Status(_) => None,
                StateTableEntry::Slots(slots) => Some((offset, slots)),
            })
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(source_offset) =
                entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(offset))
            else {
                continue;
            };
            let Ok(frame) = crate::om::state_slot_lane::StateSlotLane::new(source_offset, slots)
            else {
                continue;
            };

            output.push(OmOperationStateSlotLane {
                id: om_padded_state_id(
                    "nx:feature-history:operation-state-slot-lane#",
                    section_ordinal,
                    ordinal,
                ),
                section_link: link.id.clone(),
                ordinal,
                frame,
                source_entry: entry.name.clone(),
            });
        }
    }
    Ok(output)
}

/// Unit declared by an NX numeric expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExpressionUnit {
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
    pub(super) fn property_name(&self) -> String {
        let name = match self {
            Self::Millimeter => "millimeter",
            Self::Inch => "inch",
            Self::Degree => "degree",
            Self::Native(unit) => unit.as_str(),
        };
        name.to_owned()
    }
}

pub(super) fn expression_length_in_millimeters(unit: &ExpressionUnit, value: f64) -> Option<f64> {
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ExpressionDeclarationWire")]
pub(super) struct ExpressionDeclaration {
    /// Globally unique declaration identity.
    pub(super) id: String,
    /// Persistent OM object identifier.
    pub(super) object_id: u32,
    /// Owning entry in the native OM record directory.
    pub(super) record: String,
    /// Exact NX parameter name.
    pub(super) name: ParameterName<String, u32>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) literal: Option<String>,
    /// Directory entry containing the declaration record.
    pub(super) source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    pub(super) source_offset: u64,
}

#[cfg(test)]
mod expression_wire_tests;
#[cfg(test)]
mod record_wire_tests;

#[derive(Serialize)]
struct ExpressionDeclarationRef<'a> {
    id: &'a str,
    object_id: u32,
    record: &'a str,
    name: &'a str,
    parameter_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    qualifier: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    literal: Option<&'a str>,
    source_entry: &'a str,
    source_offset: u64,
}

impl Serialize for ExpressionDeclaration {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ExpressionDeclarationRef {
            id: &self.id,
            object_id: self.object_id,
            record: &self.record,
            name: self.name.as_str(),
            parameter_index: self.name.index(),
            qualifier: self.name.qualifier(),
            literal: self.literal.as_deref(),
            source_entry: &self.source_entry,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_qualifier"
    )]
    qualifier: Option<String>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_literal"
    )]
    literal: Option<String>,
    /// Directory entry containing the declaration record.
    source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    source_offset: u64,
}

#[cfg(test)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ExpressionWire")]
pub(super) struct ParameterFormula {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Externally bounded OM record and its persistent identity.
    pub(super) owner: Option<ExpressionOwner>,
    /// Exact-name declaration record for this parameter, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) declaration: Option<String>,
    /// NX parameter name.
    pub(super) name: ParameterName<String>,
    /// Declared native unit.
    pub(super) unit: ExpressionUnit,
    /// Exact serialized expression text.
    pub(super) expression: String,
    /// Finite numeric value after context-free and dependency-graph evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) value: Option<FiniteReal>,
    /// Directory entry containing the OM section.
    pub(super) source_entry: String,
    /// Self-contained expression table selected by the nearest preceding table marker.
    pub(super) source_table: cadmpeg_core::text::NonBlankString,
    /// Absolute file offset of the expression text.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct ExpressionRef<'a> {
    id: &'a str,
    object_id: Option<u32>,
    record: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declaration: Option<&'a str>,
    name: &'a str,
    parameter_index: Option<u32>,
    qualifier: Option<&'a str>,
    unit: &'a ExpressionUnit,
    expression: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    source_entry: &'a str,
    source_table: &'a str,
    source_offset: u64,
}

impl Serialize for ParameterFormula {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ExpressionRef {
            id: &self.id,
            object_id: self.owner.as_ref().map(|owner| owner.object_id),
            record: self.owner.as_ref().map(|owner| owner.record.as_str()),
            declaration: self.declaration.as_deref(),
            name: self.name.as_str(),
            parameter_index: self.name.index(),
            qualifier: self.name.qualifier(),
            unit: &self.unit,
            expression: &self.expression,
            value: self.value.map(FiniteReal::get),
            source_entry: &self.source_entry,
            source_table: self.source_table.as_str(),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExpressionOwner {
    pub(super) object_id: u32,
    pub(super) record: String,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_declaration"
    )]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    value: Option<f64>,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Self-contained expression table selected by the nearest preceding table marker.
    #[serde(default)]
    source_table: String,
    /// Absolute file offset of the expression text.
    source_offset: u64,
}

#[cfg(test)]
impl From<ParameterFormula> for ExpressionWire {
    fn from(value: ParameterFormula) -> Self {
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
            value: value.value.map(FiniteReal::get),
            source_entry: value.source_entry,
            source_table: value.source_table.as_str().to_owned(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<ExpressionWire> for ParameterFormula {
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
            value: wire
                .value
                .map(|value| FiniteReal::new(value).ok_or("expression value must be finite"))
                .transpose()?,
            source_entry: wire.source_entry,
            source_table: cadmpeg_core::text::NonBlankString::new(wire.source_table)
                .ok_or("source_table must not be empty")?,
            source_offset: wire.source_offset,
        })
    }
}

/// Iterate exact `p<decimal>[_qualifier]` references in formula occurrence order.
pub(crate) fn expression_parameter_names(expression: &str) -> impl Iterator<Item = &str> + '_ {
    let bytes = expression.as_bytes();
    let mut at = 0usize;
    std::iter::from_fn(move || {
        while at < bytes.len() {
            let Some(end) = expression_parameter_reference_end(bytes, at) else {
                at += 1;
                continue;
            };
            let name = &expression[at..end];
            at = end;
            return Some(name);
        }
        None
    })
}

pub(crate) fn evaluate_parameterized_expression(
    expression: &str,
    mut parameter_value: impl FnMut(&str) -> Option<f64>,
) -> Option<FiniteReal> {
    let bytes = expression.as_bytes();

    let mut substituted = String::new();
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ClassDefinitionWire")]
pub(super) struct ClassDefinition {
    /// Globally unique native-record identity.
    id: String,
    /// Registered `UGS::` class name.
    name: String,
    /// Zero-based declaration ordinal used as class identity.
    ordinal: u32,
    /// First registry-token byte serialized after the class name (legacy field name).
    trailing_code: u8,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registry_suffix: NativeBytes<Vec<u8>>,
    /// Absolute file offset of the containing OM section base.
    section_offset: u64,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the definition's length byte.
    source_offset: u64,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_registry_storage_code"
    )]
    registry_storage_code: Option<u32>,
    /// One-based base-class ordinal from the complete class registry tail.
    /// Zero denotes the registry root.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_registry_base_class"
    )]
    registry_base_class: Option<u32>,
    /// One-based reference-list ordinal from the complete class registry tail.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_registry_reference"
    )]
    registry_reference: Option<u32>,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registry_suffix: NativeBytes<Vec<u8>>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layout_prefix: NativeBytes<Vec<u8>>,
    /// Stable eight-byte class fingerprint in a framed registry suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_schema_fingerprint"
    )]
    schema_fingerprint: Option<NativeBytes<[u8; 8]>>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_layout_terminal"
    )]
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
            registry_suffix: (value.registry_suffix),
            layout_prefix: (layout
                .as_ref()
                .map_or_else(Vec::new, |layout| layout.prefix.to_vec()))
            .into(),
            schema_fingerprint: (registry
                .map(|layout| layout.schema_fingerprint)
                .or_else(|| layout.as_ref().map(|layout| layout.fingerprint)))
            .map(Into::into),
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
            registry_suffix: (wire.registry_suffix),
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FieldDefinitionWire")]
pub(super) struct FieldDefinition {
    /// Globally unique declaration identity.
    id: String,
    /// Registered `m_` member name.
    name: String,
    /// Zero-based declaration ordinal within its section.
    ordinal: u32,
    /// First registry-token byte serialized immediately after the name (legacy field name).
    trailing_code: u8,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registry_suffix: NativeBytes<Vec<u8>>,
    /// Absolute file offset of the containing OM section signature.
    section_offset: u64,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the declaration length byte.
    source_offset: u64,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_registry_storage_code"
    )]
    registry_storage_code: Option<u32>,
    /// One-based declaring-class ordinal from the complete member registry head.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_registry_owner_class"
    )]
    registry_owner_class: Option<u32>,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registry_suffix: NativeBytes<Vec<u8>>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layout_prefix: NativeBytes<Vec<u8>>,
    /// Stable eight-byte field fingerprint in a framed registry suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_schema_fingerprint"
    )]
    schema_fingerprint: Option<NativeBytes<[u8; 8]>>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_layout_terminal"
    )]
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
            registry_suffix: (value.registry_suffix),
            layout_prefix: (layout
                .as_ref()
                .map_or_else(Vec::new, |layout| layout.prefix.to_vec()))
            .into(),
            schema_fingerprint: (layout.as_ref().map(|layout| layout.fingerprint)).map(Into::into),
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
            registry_suffix: (wire.registry_suffix),
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ObjectRecordWire")]
pub(super) struct ObjectRecord {
    /// Globally unique record identity.
    id: String,
    /// Persistent OM object identifier and the offset of its table word.
    object_id: (u32, u64),
    /// Zero-based indexed-section ordinal within the container.
    section_ordinal: u32,
    /// Zero-based record ordinal within the indexed section.
    record_ordinal: u32,
    /// Absolute file offset of the containing OM section base.
    section_offset: u64,
    /// Exact serialized record length.
    byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// Content-backed identity when the scoped exact bytes are unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stable_identity: Option<String>,
    /// Ordered distinct same-section records referenced by this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
    /// Ordered distinct same-section records that reference this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependents: Vec<String>,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the record start.
    source_offset: u64,
}

#[derive(Serialize)]
struct ObjectRecordRef<'a> {
    id: &'a str,
    object_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_id_source_offset: Option<u64>,
    section_ordinal: u32,
    record_ordinal: u32,
    section_offset: u64,
    byte_len: u64,
    sha256: &'a cadmpeg_ir::hash::digest::Sha256Digest,
    #[serde(skip_serializing_if = "Option::is_none")]
    stable_identity: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: &'a Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependents: &'a Vec<String>,
    source_entry: &'a str,
    source_offset: u64,
}

impl Serialize for ObjectRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ObjectRecordRef {
            id: &self.id,
            object_id: Some(self.object_id.0),
            object_id_source_offset: Some(self.object_id.1),
            section_ordinal: self.section_ordinal,
            record_ordinal: self.record_ordinal,
            section_offset: self.section_offset,
            byte_len: self.byte_len,
            sha256: &self.sha256,
            stable_identity: self.stable_identity.as_deref(),
            dependencies: &self.dependencies,
            dependents: &self.dependents,
            source_entry: &self.source_entry,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

// The identity and its offset are stated together, so the refusal names which
// half the document left unstated.
cadmpeg_core::named_optional_field!(
    deserialize_object_id_source_offset,
    u64,
    "object_id_source_offset"
);

#[derive(Serialize, Deserialize)]
struct ObjectRecordWire {
    id: String,
    object_id: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_object_id_source_offset"
    )]
    object_id_source_offset: Option<u64>,
    section_ordinal: u32,
    record_ordinal: u32,
    section_offset: u64,
    byte_len: u64,
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stable_identity"
    )]
    stable_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependents: Vec<String>,
    source_entry: String,
    source_offset: u64,
}

#[cfg(test)]
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
fn stable_object_record_identity(source_entry: &str, bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"nx:om:object-record\0");
    digest.update(source_entry.as_bytes());
    digest.update([0]);
    digest.update(bytes);
    let digest: [u8; 32] = digest.finalize().into();
    data_block_hex(&digest, "nx:om:object-record:")
}

/// Return position-independent identities for one indexed object-record graph.
///
/// `RecordOrdinal16` values are local to one indexed section. The canonical
/// graph replaces those values with links to records in traversal order, so a
/// directory reorder does not change the identity. The traversal is explicit
/// rather than recursive because malformed or adversarial records must not
/// turn identity extraction into a stack overflow. Persistent handles remain
/// serialized bytes: no cross-record owner relation proves that they are
/// position-independent. A local work cap shared by the section's records
/// returns no identity when canonicalization would exceed it.
fn stable_object_record_identities(source_entry: &str, records: &[&[u8]]) -> Vec<Option<String>> {
    const MAX_GRAPH_WORK: usize = 8 * 1024 * 1024;

    let mut references = Vec::new();
    for bytes in records {
        let parsed = crate::om::counted_record_references(bytes, 0, records.len());
        references.push(
            parsed
                .into_iter()
                .map(|reference| (reference.offset, usize::from(reference.value)))
                .collect::<Vec<_>>(),
        );
    }
    let mut graph_work = MAX_GRAPH_WORK;

    let mut identities = Vec::new();
    for root in 0..records.len() {
        let identity = if references[root].is_empty() {
            Some(stable_object_record_identity(source_entry, records[root]))
        } else {
            stable_object_record_graph_identity(
                source_entry,
                records,
                &references,
                root,
                &mut graph_work,
            )
        };
        identities.push(identity);
    }
    identities
}

fn consume_stable_object_graph_work(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.checked_sub(amount)?;
    Some(())
}

fn append_stable_object_graph_node(
    digest: &mut Sha256,
    graph_work: &mut usize,
    node_id: u64,
) -> Option<()> {
    consume_stable_object_graph_work(graph_work, 9)?;
    digest.update([STABLE_GRAPH_NODE_START]);
    digest.update(node_id.to_le_bytes());
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

    consume_stable_object_graph_work(graph_work, source_entry.len().checked_add(32)?)?;
    let mut digest = Sha256::new();
    digest.update(b"nx:om:object-record-graph\0");
    digest.update(cadmpeg_core::decode::u64_from_index(source_entry.len()).to_le_bytes());
    digest.update(source_entry.as_bytes());

    let mut next_node_id = 0_u64;
    let mut node_ids = BTreeMap::<usize, u64>::new();
    node_ids.insert(root, next_node_id);
    append_stable_object_graph_node(&mut digest, graph_work, next_node_id)?;
    next_node_id = next_node_id.checked_add(1)?;
    let mut stack = vec![Frame {
        record: root,
        next_reference: 0,
        raw_cursor: 0,
    }];

    while let Some(frame_index) = stack.len().checked_sub(1) {
        let (record, next_reference, raw_cursor) = {
            let frame = stack.get(frame_index)?;
            (frame.record, frame.next_reference, frame.raw_cursor)
        };
        let &record_bytes = records.get(record)?;
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
            let raw_work = raw.len().checked_add(9)?;
            consume_stable_object_graph_work(graph_work, raw_work)?;
            digest.update([RAW]);
            digest.update(cadmpeg_core::decode::u64_from_index(raw.len()).to_le_bytes());
            digest.update(raw);

            let frame = stack.get_mut(frame_index)?;
            let next_reference = frame.next_reference.checked_add(1)?;
            frame.next_reference = next_reference;
            frame.raw_cursor = reference_end;

            if let Some(&target_id) = node_ids.get(&target) {
                consume_stable_object_graph_work(graph_work, 9)?;
                digest.update([REFERENCE_BACK]);
                digest.update(target_id.to_le_bytes());
            } else {
                node_ids.insert(target, next_node_id);
                digest.update([REFERENCE_NEW]);
                digest.update(next_node_id.to_le_bytes());
                append_stable_object_graph_node(&mut digest, graph_work, next_node_id)?;
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
            let raw_work = raw.len().checked_add(1)?;
            consume_stable_object_graph_work(graph_work, raw_work)?;
            digest.update([RAW]);
            digest.update(cadmpeg_core::decode::u64_from_index(raw.len()).to_le_bytes());
            digest.update(raw);
            digest.update([NODE_END]);
            stack.pop();
        }
    }

    let digest: [u8; 32] = digest.finalize().into();
    Some(data_block_hex(&digest, "nx:om:object-record:"))
}

/// Counted active-object membership table from `RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "membership_wire::TableWire")]
pub(super) struct RmFastLoadObjectIdTable {
    /// Globally unique table identity.
    id: String,
    /// Ordered members in the native `rmfastload_object_ids` arena.
    members: ObjectIdMembers<String>,
    /// Directory entry containing the table.
    source_entry: String,
    /// Absolute file offset of the `UGS::Solid::Topol` registry marker.
    registry_source_offset: u64,
    /// Absolute file offset of the four-byte count word.
    source_offset: u64,
}

/// One fixed-width active-object membership word from `RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "membership_wire::MemberWire")]
pub(super) struct RmFastLoadObjectId {
    /// Globally unique member identity.
    id: String,
    /// Owning table in the native `rmfastload_object_id_tables` arena.
    table: String,
    /// Zero-based serialized member order.
    ordinal: u32,
    /// Decoded active object identifier.
    value: u32,
    /// Record-order-independent identity when the value is unique in the table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stable_identity: Option<String>,
    /// Absolute file offset of the four-byte object-id word.
    source_offset: u64,
}

impl RmFastLoadObjectIdTable {
    fn raw_count(&self) -> [u8; 4] {
        self.members.count().to_le_bytes()
    }
}
impl RmFastLoadObjectId {
    fn raw(&self) -> [u8; 4] {
        self.value.to_le_bytes()
    }
}

/// One externally bounded block in an NX OM offset-only column store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DataBlock {
    /// Globally unique block identity.
    pub(super) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(super) section_ordinal: u32,
    /// Zero-based block ordinal within the offset-only section.
    pub(super) block_ordinal: u32,
    /// Whether this is the store control block or one data column block.
    pub(super) role: DataBlockRole,
    /// Absolute file offset of the containing OM section base.
    pub(super) section_offset: u64,
    /// Exact serialized block length.
    pub(super) byte_len: u64,
    /// SHA-256 of the exact serialized block bytes.
    pub(super) sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// Content-backed identity when the scoped exact bytes are unique.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stable_identity"
    )]
    pub(super) stable_identity: Option<String>,
    /// Directory entry containing the OM section.
    pub(super) source_entry: String,
    /// Absolute file offset of the block start.
    pub(super) source_offset: u64,
}

/// Admitted complete grammar selected for one offset-store control block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataBlockControlFormKind {
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DataBlockControlFormWire")]
pub(super) struct DataBlockControlForm {
    /// Globally unique control-form identity.
    pub(super) id: String,
    /// Opening control block in the native `data_blocks` arena.
    data_block: String,
    /// Selected complete control grammar.
    kind: DataBlockControlFormKind,
    /// Absolute file offset of the control block.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DataBlockControlFormRef<'a> {
    id: &'a str,
    data_block: &'a str,
    kind: DataBlockControlFormKindWire,
    value_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    leading_value_width: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    leading_value: Option<u32>,
    byte_len: u64,
    source_offset: u64,
}

impl Serialize for DataBlockControlForm {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (kind, leading_value_width, leading_value) = match self.kind {
            DataBlockControlFormKind::ZeroPrefixed { .. } => {
                (DataBlockControlFormKindWire::ZeroPrefixed, None, None)
            }
            DataBlockControlFormKind::ProductAnchored { leading, .. } => (
                DataBlockControlFormKindWire::ProductAnchored,
                leading.map(ControlLeadingValue::width),
                leading.map(ControlLeadingValue::value),
            ),
        };
        DataBlockControlFormRef {
            id: &self.id,
            data_block: &self.data_block,
            kind,
            value_count: self.kind.value_count(),
            leading_value_width,
            leading_value,
            byte_len: self.kind.byte_len(),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

impl DataBlockControlFormKind {
    fn value_count(self) -> u32 {
        match self {
            Self::ZeroPrefixed { value_count } | Self::ProductAnchored { value_count, .. } => {
                value_count.get()
            }
        }
    }

    fn byte_len(self) -> u64 {
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_leading_value_width"
    )]
    leading_value_width: Option<u8>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_leading_value"
    )]
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

#[cfg(test)]
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
pub(super) struct DataBlockControlValue {
    /// Globally unique control-value identity.
    pub(super) id: String,
    /// Owning control block in the native `data_blocks` arena.
    data_block: String,
    /// Zero-based word order in the complete control block.
    ordinal: u32,
    /// Unsigned 24-bit value serialized after the zero byte.
    value: crate::om::control_word::ControlWord24,
    /// Absolute file offset of the four-byte word.
    pub(super) source_offset: u64,
}

/// Ordered little-endian value preceding a store product anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DataBlockControlIndexValue {
    /// Globally unique value identity.
    pub(super) id: String,
    /// Control block that opens the logical lane in the native `data_blocks` arena.
    data_block: String,
    /// Zero-based value order in the aligned prefix array.
    ordinal: u32,
    /// Unsigned little-endian value.
    value: u32,
    /// Same-section offset-store block addressed by an in-range value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_data_block"
    )]
    target_data_block: Option<String>,
    /// Absolute file offset of the four-byte value.
    pub(super) source_offset: u64,
}

/// Registered class selected by the leading lane of an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DataBlockControlClassReferenceWire")]
pub(super) struct DataBlockControlClassReference {
    /// Globally unique class-reference identity.
    pub(super) id: String,
    /// Owning control block in the native `data_blocks` arena.
    data_block: String,
    /// Zero-based order in the class-selection lane.
    ordinal: u32,
    /// Zero-based ordinal in the store's class registry.
    class_ordinal: u32,
    /// Retained class definition and name when that registry slot exists.
    class: Option<DataBlockControlClassRef>,
    /// Absolute file offset of the four-byte control word.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DataBlockControlClassReferenceRef<'a> {
    id: &'a str,
    data_block: &'a str,
    ordinal: u32,
    class_ordinal: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_definition: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_name: Option<&'a str>,
    source_offset: u64,
}

impl Serialize for DataBlockControlClassReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DataBlockControlClassReferenceRef {
            id: &self.id,
            data_block: &self.data_block,
            ordinal: self.ordinal,
            class_ordinal: self.class_ordinal,
            class_definition: self.class.as_ref().map(|class| class.definition.as_str()),
            class_name: self.class.as_ref().map(|class| class.name.as_str()),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

/// Retained class-definition identity and registered name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DataBlockControlClassRef {
    definition: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct DataBlockControlClassReferenceWire {
    id: String,
    data_block: String,
    ordinal: u32,
    class_ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_class_definition"
    )]
    class_definition: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_class_name"
    )]
    class_name: Option<String>,
    source_offset: u64,
}

#[cfg(test)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DataBlockReferenceWire")]
pub(super) struct DataBlockReference {
    /// Globally unique reference identity.
    pub(super) id: String,
    /// Owning block in the native `data_blocks` arena.
    pub(super) data_block: String,
    /// Zero-based reference order within the block.
    pub(super) ordinal: u32,
    /// Referenced persistent OM object ID.
    pub(super) object: crate::om::reference_index::FeatureReferenceToken,
    /// Uniquely resolved object record in the same directory entry.
    pub(super) target_record: Option<String>,
    /// Uniquely resolved parameter declaration carrying this object ID.
    pub(super) target_expression_declaration: Option<String>,
    /// Absolute file offset of the object-index token.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DataBlockReferenceRef<'a> {
    id: &'a str,
    data_block: &'a str,
    ordinal: u32,
    object_id: u32,
    raw_object_id: NativeBytes<&'a [u8]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_record: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_expression_declaration: Option<&'a str>,
    source_offset: u64,
}

impl Serialize for DataBlockReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DataBlockReferenceRef {
            id: &self.id,
            data_block: &self.data_block,
            ordinal: self.ordinal,
            object_id: self.object.value(),
            raw_object_id: (self.object.raw()).into(),
            target_record: self.target_record.as_deref(),
            target_expression_declaration: self.target_expression_declaration.as_deref(),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DataBlockReferenceWire {
    /// Globally unique reference identity.
    id: String,
    /// Owning block in the native `data_blocks` arena.
    data_block: String,
    /// Zero-based reference order within the block.
    ordinal: u32,
    /// Referenced persistent OM object ID.
    object_id: u32,
    /// Exact serialized object-index token.
    raw_object_id: NativeBytes<Vec<u8>>,
    /// Uniquely resolved object record in the same directory entry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_record"
    )]
    target_record: Option<String>,
    /// Uniquely resolved parameter declaration carrying this object ID.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_expression_declaration"
    )]
    target_expression_declaration: Option<String>,
    /// Absolute file offset of the object-index token.
    source_offset: u64,
}

#[cfg(test)]
impl From<DataBlockReference> for DataBlockReferenceWire {
    fn from(value: DataBlockReference) -> Self {
        Self {
            id: value.id,
            data_block: value.data_block,
            ordinal: value.ordinal,
            object_id: value.object.value(),
            raw_object_id: (value.object.raw().to_vec()).into(),
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "color_wire::PartColorTableWire")]
pub(super) struct PartColorTable {
    /// Globally unique table identity.
    id: String,
    /// Registered `UGS::COLOR_table` declaration in `class_definitions`.
    class_definition: String,
    /// Exact background components and their absolute file offsets.
    background: [(ColorComponent, u64); 3],
    /// Ordered entries in the native `part_color_definitions` arena.
    definitions: [String; PALETTE_SIZE],
    /// Directory entry containing the table.
    source_entry: String,
    /// Absolute file offset of the counted name roster.
    source_offset: u64,
}

/// One named RGB entry from an NX part palette.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "color_wire::PartColorDefinitionWire")]
pub(super) struct PartColorDefinition {
    /// Globally unique color-definition identity.
    pub(super) id: String,
    /// Owning table in the native `part_color_tables` arena.
    pub(super) color_table: String,
    /// One-based NX color index.
    pub(super) color_index: PaletteIndex,
    /// Serialized color name.
    pub(super) name: String,
    /// Exact normalized components and their absolute file offsets.
    pub(super) components: [(ColorComponent, u64); 3],
    /// Absolute file offset of the opening `05` marker.
    pub(super) source_offset: u64,
}

/// Complete composite table spanning linked and target-index row grammars.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DataBlockColumnIndexTable {
    /// Globally unique table identity.
    pub(super) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(super) section_ordinal: u32,
    /// Leading mode-7 linked row.
    pub(super) opening_linked_row: String,
    /// Consecutive target and linked rows with their checked index interval.
    #[serde(flatten)]
    pub(super) rows: ColumnIndexRows,
    /// Directory entry containing the store.
    pub(super) source_entry: String,
    /// Absolute source offset of the opening linked row.
    pub(super) source_offset: u64,
}

/// Product/version header from one indexed NX OM store.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "StoreHeaderWire")]
pub(super) enum StoreHeader {
    /// Header in an ID-bounded store record.
    Fixed(FixedStoreHeader),
    /// Header in an offset-bounded store block.
    OffsetOnly(OffsetStoreHeader),
}

#[derive(Serialize)]
struct StoreHeaderRef<'a> {
    object_id: Option<u32>,
    #[serde(flatten)]
    header: &'a OffsetStoreHeader,
}

impl Serialize for StoreHeader {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StoreHeaderRef {
            object_id: match self {
                Self::Fixed(value) => Some(value.object_id),
                Self::OffsetOnly(_) => None,
            },
            header: self.header(),
        }
        .serialize(serializer)
    }
}

/// Product/version header in an ID-bounded store record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::native) struct FixedStoreHeader {
    /// Persistent object identity.
    object_id: u32,
    /// Product/version location and text.
    header: OffsetStoreHeader,
}

/// Product/version location and text in an offset-bounded store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct OffsetStoreHeader {
    /// Globally unique store-header identity.
    pub(super) id: String,
    /// Zero-based indexed-section ordinal within the container.
    section_ordinal: u32,
    /// Exact printable product/version text.
    version: crate::om::product::ProductText<String>,
    /// Directory entry containing the OM store.
    source_entry: String,
    /// Absolute file offset of the `04 01` marker.
    pub(super) source_offset: u64,
}

impl StoreHeader {
    pub(super) fn header(&self) -> &OffsetStoreHeader {
        match self {
            Self::Fixed(header) => &header.header,
            Self::OffsetOnly(header) => header,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoreHeaderWire {
    object_id: Option<u32>,
    #[serde(flatten)]
    header: OffsetStoreHeader,
}

impl From<StoreHeaderWire> for StoreHeader {
    fn from(wire: StoreHeaderWire) -> Self {
        match wire.object_id {
            Some(object_id) => Self::Fixed(FixedStoreHeader {
                object_id,
                header: wire.header,
            }),
            None => Self::OffsetOnly(wire.header),
        }
    }
}

#[cfg(test)]
impl From<StoreHeader> for StoreHeaderWire {
    fn from(header: StoreHeader) -> Self {
        match header {
            StoreHeader::Fixed(header) => Self {
                object_id: Some(header.object_id),
                header: header.header,
            },
            StoreHeader::OffsetOnly(header) => Self {
                object_id: None,
                header,
            },
        }
    }
}

/// Role of one bounded block in an offset-only NX OM store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DataBlockRole {
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
fn data_block_digest(source_entry: &str, role: DataBlockRole, bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"nx:om:data-block\0");
    digest.update(source_entry.as_bytes());
    digest.update([
        0,
        match role {
            DataBlockRole::Control => 0,
            DataBlockRole::Column => 1,
        },
    ]);
    digest.update(bytes);
    digest.finalize().into()
}

fn data_block_hex(digest: &[u8; 32], prefix: &str) -> String {
    format!("{prefix}{}", cadmpeg_ir::hash::LowerHex(digest))
}

/// Self-framed printable string carried by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StringValue {
    /// Globally unique value identity.
    id: String,
    /// Owning entry in the native OM record directory.
    record: String,
    /// Persistent OM object identifier.
    object_id: u32,
    /// Zero-based occurrence ordinal within the owning record.
    ordinal: u32,
    /// Exact printable value.
    value: PrintableString<String>,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the `66 32 03` marker.
    source_offset: u64,
}

#[cfg(test)]
mod printable_value_wire_tests {
    use super::StringValue;

    #[test]
    fn object_record_requires_identity_and_its_offset() {
        let wire = serde_json::json!({
            "id": "record", "object_id": 1, "object_id_source_offset": 10,
            "section_ordinal": 0, "record_ordinal": 0, "section_offset": 0,
            "byte_len": 1, "sha256": "d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa", "source_entry": "entry", "source_offset": 20
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
pub(super) struct ObjectReference {
    /// Globally unique occurrence identity.
    id: String,
    /// Owning entry in the native OM record directory.
    record: String,
    /// Persistent OM object identifier.
    object_id: u32,
    /// Zero-based occurrence ordinal within the owning record.
    ordinal: u32,
    /// Typed reference and its same-section target when present.
    #[serde(flatten, with = "reference_wire")]
    reference: RecordReference<String>,
    /// Directory entry containing the OM section.
    source_entry: String,
    /// Absolute file offset of the reference marker.
    source_offset: u64,
}

/// Exact two-token persistent-handle run in one bounded OM object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ObjectRecordHandlePair {
    /// Globally unique pair identity.
    pub(super) id: String,
    /// Owning entry in the native OM record directory.
    record: String,
    /// Persistent OM object identifier.
    object_id: u32,
    /// First handle-reference occurrence.
    first_reference: String,
    /// Second handle-reference occurrence.
    second_reference: String,
    /// First persistent-handle value.
    first_handle: u32,
    /// Second persistent-handle value.
    second_handle: u32,
    /// Absolute file offset of the first `e0` marker.
    pub(super) source_offset: u64,
}

/// Ordered persistent or tagged reference in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DataBlockControlReference {
    /// Globally unique occurrence identity.
    pub(super) id: String,
    /// Owning control block in the native `data_blocks` arena.
    data_block: String,
    /// Zero-based retained-reference order within the control block.
    ordinal: u32,
    /// Self-identifying reference payload.
    #[serde(flatten)]
    reference: DirectReference,
    /// Absolute file offset of the reference marker.
    pub(super) source_offset: u64,
}

/// Exact two-token persistent-handle run in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DataBlockControlHandlePair {
    /// Globally unique pair identity.
    pub(super) id: String,
    /// Owning control block in the native `data_blocks` arena.
    data_block: String,
    /// First handle-reference occurrence.
    first_reference: String,
    /// Second handle-reference occurrence.
    second_reference: String,
    /// First persistent-handle value.
    first_handle: u32,
    /// Second persistent-handle value.
    second_handle: u32,
    /// Absolute file offset of the first `e0` marker.
    pub(super) source_offset: u64,
}

/// Cross-record identity established by equal persistent-handle values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistentHandle {
    /// Globally unique handle identity.
    id: String,
    /// Unsigned persistent-handle value.
    value: u32,
    /// Ordered distinct OM directory records containing the handle.
    records: Vec<String>,
    /// Total serialized occurrences across OM records and offset-store control blocks.
    occurrence_count: u32,
    /// Ordered distinct offset-store control blocks containing the handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    data_blocks: Vec<String>,
    /// Ordered distinct EXTREFSTREAM records containing the same handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    external_records: Vec<String>,
    /// Total serialized occurrences across EXTREFSTREAM record prefixes and tails.
    #[serde(default)]
    external_occurrence_count: u32,
}

/// Named NX arrangement from `/Root/part/arrangements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Configuration {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Arrangement name.
    pub(super) name: String,
    /// Whether NX marks this arrangement as the default.
    is_default: bool,
    /// Directory entry containing the arrangement XML.
    source_entry: String,
    /// Absolute file offset of the arrangement element.
    pub(super) source_offset: u64,
}

/// Exact agreement between the default arrangement and the part attribute
/// naming the active arrangement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ConfigurationAttributeUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Default arrangement from the native configuration arena.
    pub(super) configuration: String,
    /// Typed `NX_Arrangement` part attribute carrying the same name.
    part_attribute: String,
    /// Exact shared arrangement name.
    name: String,
}

/// One typed part-level attribute from `/Root/part/attrs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PartAttribute {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Attribute owner token.
    owner: String,
    /// UTF-8 attribute title.
    pub(super) title: String,
    /// UTF-8 attribute value.
    pub(super) value: String,
    /// XML schema type token.
    value_type: String,
    /// Whether product-data management owns the value.
    pdm_based: bool,
    /// Attribute record schema version.
    version: u32,
    /// Directory entry containing the attribute XML.
    source_entry: String,
    /// Absolute file offset of the attribute element.
    pub(super) source_offset: u64,
}

/// End-anchored child-part string from an NX external-reference stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReference {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based string-table ordinal within the stream.
    ordinal: u32,
    /// Exact serialized child-part name or path.
    path: String,
    /// Directory entry containing the external-reference stream.
    source_entry: String,
    /// Absolute file offset of the first path byte.
    pub(super) source_offset: u64,
}

/// Externally bounded record retained from an EXTREFSTREAM index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReferenceIndexedRecord {
    /// Globally unique indexed-record identity.
    id: String,
    /// Record type from the external-reference directory.
    record_id: u32,
    /// Exact serialized record length.
    byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// Specialized handle-set record when that complete grammar resolves.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_handle_set_record"
    )]
    handle_set_record: Option<String>,
    /// Directory entry containing the external-reference stream.
    source_entry: String,
    /// Absolute file offset of the indexed record.
    source_offset: u64,
}

/// Indexed EXTREFSTREAM record prefix with its exact handle membership set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReferenceRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Record type from the external-reference directory.
    record_id: u32,
    /// Count declared before the four ID slots.
    declared_count: u16,
    /// Four uninterpreted little-endian ID slots.
    id_slots: [u32; 4],
    /// Ordered encoded handle tokens with derived closing and length fields.
    #[serde(flatten)]
    handles: ExtrefHandles,
    /// Length after the decoded handle-set prefix and before the next record or string table.
    tail_byte_len: u64,
    /// Directory entry containing the external-reference stream.
    source_entry: String,
    /// Absolute file offset of the record marker.
    pub(super) source_offset: u64,
}

/// Empty EXTREFSTREAM indexed-record form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReferenceEmptyRecord {
    /// Globally unique empty-record identity.
    id: String,
    /// Owning record in the native `external_reference_indexed_records` arena.
    indexed_record: String,
    /// Whether the six-byte header is followed by a closing `01` marker.
    closing_marker: bool,
}

/// Exact adjacent reference pair in an EXTREFSTREAM handle-set tail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReferenceTailReferencePair {
    /// Globally unique pair identity.
    id: String,
    /// Owning record in the native `external_reference_records` arena.
    handle_set_record: String,
    /// Zero-based pair order within the bounded tail.
    ordinal: u32,
    /// Persistent handle from the `e0 + u32 BE` token.
    persistent_handle: u32,
    /// Low 28 bits of the following four-byte `0xC?` reference.
    tagged_reference: crate::om::reference_value::Tagged28,
    /// Absolute file offset of the `e0` marker.
    source_offset: u64,
}

/// One external-reference record slot resolved through its same-stream string table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReferenceRecordStringUse {
    /// Globally unique slot-use identity.
    id: String,
    /// Owning record in the native `external_reference_records` arena.
    external_record: String,
    /// Zero-based slot in the record's four-value lane.
    slot: ExtrefSlot,
    /// Serialized string-table index.
    string_index: u32,
    /// Target in the native `external_references` arena.
    external_reference: String,
    /// Absolute file offset of the serialized `u32 LE` slot value.
    source_offset: u64,
}

/// Child-part identity selected by one complete external-reference record lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExternalReferenceRecordChild {
    /// Globally unique child-binding identity.
    id: String,
    /// Owning record in the native `external_reference_records` arena.
    external_record: String,
    /// Slot-zero child filename in the native `external_references` arena.
    name_reference: String,
    /// Slot-two child directory in the native `external_references` arena.
    directory_reference: String,
}

/// Exact QAF catalog mapping for one embedded material texture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MaterialTextureCatalogEntry {
    /// Globally unique native relation identity.
    pub(super) id: String,
    /// Target in the native `material_texture_assets` arena.
    texture_asset: String,
    /// Stored path relative to `/Root/`.
    storage_path: String,
    /// Logical material-texture path recorded by QAF metadata.
    material_path: String,
    /// Exact QAF creation-time text.
    create_time: String,
    /// Exact QAF modification-time text.
    modify_time: String,
    /// Directory entry containing the QAF catalog.
    source_entry: String,
    /// Absolute file offset of the `folderProperties` element.
    pub(super) source_offset: u64,
}

/// Join QAF material paths to embedded TIFF streams by exact stored path.
pub(super) fn material_texture_catalog_entries(
    ctx: &DecodeContext<'_>,
    container: &Container,
    assets: &[MaterialTextureAsset],
) -> Result<Vec<MaterialTextureCatalogEntry>, CodecError> {
    let Some((entry_index, entry)) = container
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == "/Root/qafmetadata")
    else {
        return Ok(Vec::new());
    };
    let Some((entry_offset, size)) = entry.file_span() else {
        return Ok(Vec::new());
    };
    let Some(start) = usize::try_from(entry_offset).ok() else {
        return Ok(Vec::new());
    };
    let Some(size) = usize::try_from(size).ok() else {
        return Ok(Vec::new());
    };
    let Some(end) = start.checked_add(size) else {
        return Ok(Vec::new());
    };
    let Some(payload) = container.data.get(start..end) else {
        return Ok(Vec::new());
    };
    let Some(entries) = parse_material_texture_catalog(
        ctx,
        payload,
        entry_index,
        &entry.name,
        entry_offset,
        assets,
    )?
    else {
        return Ok(Vec::new());
    };
    Ok(entries)
}

fn parse_material_texture_catalog(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
    assets: &[MaterialTextureAsset],
) -> Result<Option<Vec<MaterialTextureCatalogEntry>>, CodecError> {
    let Some(xml) = xml_stream_text(payload) else {
        return Ok(None);
    };
    let admitted_document = match ctx.parse_xml(xml, "decode XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(None);
        }
    };
    let document = admitted_document.document();
    let root = document.root_element();
    if root.tag_name().name() != "folderContents" {
        return Ok(None);
    }
    let assets_by_path = assets
        .iter()
        .map(|asset| (asset.storage_path(), asset))
        .collect::<BTreeMap<_, _>>();
    let mut catalog = Vec::new();
    let mut seen_assets = BTreeSet::new();
    for node in root.children().filter(roxmltree::Node::is_element) {
        if node.tag_name().name() != "folderProperties" {
            return Ok(None);
        }
        let Some(storage_path) = node.attribute("location") else {
            return Ok(None);
        };
        let Some(material_path) = node.attribute("unmappedLocation") else {
            return Ok(None);
        };
        let mut children = node.children().filter(roxmltree::Node::is_element);
        let (Some(create), Some(modify), None) =
            (children.next(), children.next(), children.next())
        else {
            return Ok(None);
        };
        if create.tag_name().name() != "createTime" || modify.tag_name().name() != "modifyTime" {
            return Ok(None);
        }
        let Some(create_time) = create.text() else {
            return Ok(None);
        };
        let Some(modify_time) = modify.text() else {
            return Ok(None);
        };
        if !storage_path.starts_with("materialsTif/") {
            continue;
        }
        let Some(asset) = assets_by_path.get(storage_path) else {
            return Ok(None);
        };
        if material_path
            .strip_prefix("materialsTif/")
            .is_none_or(str::is_empty)
        {
            return Ok(None);
        }
        if !seen_assets.insert(asset.id.as_str()) {
            return Ok(None);
        }
        let ordinal = catalog.len();
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(node.range().start))
            .ok_or_else(|| CodecError::malformed("NX material catalog source offset overflows"))?;

        catalog.push(MaterialTextureCatalogEntry {
            id: format!("nx:qafmetadata-{entry_index}:material-texture#{ordinal}"),
            texture_asset: asset.id.clone(),
            storage_path: storage_path.to_owned(),
            material_path: material_path.to_owned(),
            create_time: create_time.to_owned(),
            modify_time: modify_time.to_owned(),
            source_entry: source_entry.to_owned(),
            source_offset,
        });
    }
    Ok(Some(catalog))
}

/// Decode end-anchored external child-part string tables.
pub(super) fn external_references(
    container: &Container,
) -> Result<Vec<ExternalReference>, cadmpeg_core::CodecError> {
    let strings = container.external_reference_strings();
    let mut ordinals = BTreeMap::<String, u32>::new();
    let mut references = Vec::new();

    for (entry, relative, path) in strings {
        let current = if let Some(ordinal) = ordinals.get_mut(entry.name.as_str()) {
            let current = *ordinal;
            *ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("NX external reference ordinal overflows"))?;
            current
        } else {
            ordinals.insert(entry.name.clone(), 1);
            0
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        references.push(ExternalReference {
            id: format!("nx:external-reference:{}#{current}", entry.name),
            ordinal: current,
            path,
            source_entry: entry.name.clone(),
            source_offset: entry_offset + cadmpeg_core::decode::u64_from_index(relative),
        });
    }
    Ok(references)
}

/// Decode exact indexed external-reference record prefixes.
pub(super) fn external_reference_records(
    container: &Container,
) -> Result<Vec<ExternalReferenceRecord>, cadmpeg_core::CodecError> {
    let parsed = container.external_reference_records();
    let mut output = Vec::new();
    for (entry, record) in parsed {
        let id = format!(
            "nx:external-reference-record:{}#{}",
            entry.name, record.record_id
        );
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
            .ok_or_else(|| {
                CodecError::malformed("NX native external reference record offset overflows")
            })?;
        output.push(ExternalReferenceRecord {
            id,
            record_id: record.record_id,
            declared_count: record.declared_count,
            id_slots: record.id_slots,
            handles: record.handles,
            tail_byte_len: cadmpeg_core::decode::u64_from_index(record.tail_byte_len),
            source_entry: entry.name.clone(),
            source_offset,
        });
    }
    Ok(output)
}

/// Retain all indexed records and link uniquely decoded handle-set records.
pub(super) fn external_reference_indexed_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
    decoded: &[ExternalReferenceRecord],
) -> Result<Vec<ExternalReferenceIndexedRecord>, cadmpeg_core::CodecError> {
    let mut decoded_by_key = BTreeMap::<(&str, u32), Option<&ExternalReferenceRecord>>::new();
    for record in decoded {
        let key = (record.source_entry.as_str(), record.record_id);
        if let Some(value) = decoded_by_key.get_mut(&key) {
            *value = None;
        } else {
            decoded_by_key.insert(key, Some(record));
        }
    }
    let parsed = container.external_reference_indexed_records();
    let mut output = Vec::new();
    for (entry, record) in parsed {
        let Some((entry_offset, _)) = entry.file_span() else {
            continue;
        };
        let Some(source_offset) =
            entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
        else {
            continue;
        };
        let byte_len = cadmpeg_core::decode::u64_from_index(record.byte_len);
        let Some(bytes) = container.bounded_entry_bytes(source_offset, byte_len) else {
            continue;
        };

        let id = format!(
            "nx:external-reference-indexed-record:{}#{}",
            entry.name, record.record_id
        );
        let handle_set_record = decoded_by_key
            .get(&(entry.name.as_str(), record.record_id))
            .and_then(|record| *record)
            .map(|record| record.id.clone());
        output.push(ExternalReferenceIndexedRecord {
            id,
            record_id: record.record_id,
            byte_len,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                bytes,
                "nx external reference indexed digest",
            )?,
            handle_set_record,
            source_entry: entry.name.clone(),
            source_offset,
        });
    }
    Ok(output)
}

/// Decode every exact six- or seven-byte empty indexed record.
pub(super) fn external_reference_empty_records(
    container: &Container,
    indexed: &[ExternalReferenceIndexedRecord],
) -> Vec<ExternalReferenceEmptyRecord> {
    let mut output = Vec::new();
    for record in indexed {
        let Some(bytes) = container.bounded_entry_bytes(record.source_offset, record.byte_len)
        else {
            continue;
        };
        let Some(closing_marker) = crate::container::parse_extref_empty_record(bytes) else {
            continue;
        };

        let id = match record.id.split_once("indexed-record") {
            Some((before, after)) => format!("{before}empty-record{after}"),
            None => record.id.clone(),
        };
        output.push(ExternalReferenceEmptyRecord {
            id,
            indexed_record: record.id.clone(),
            closing_marker,
        });
    }
    output
}

/// Decode exact adjacent reference pairs from bounded handle-set tails.
pub(super) fn external_reference_tail_reference_pairs(
    container: &Container,
    records: &[ExternalReferenceRecord],
) -> Result<Vec<ExternalReferenceTailReferencePair>, cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    for record in records {
        let Some(source_offset) =
            record
                .source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    record.handles.prefix_byte_len(),
                ))
        else {
            continue;
        };
        let Some(bytes) = container.bounded_entry_bytes(source_offset, record.tail_byte_len) else {
            continue;
        };
        for (ordinal, (offset, persistent_handle, tagged_reference)) in
            crate::container::parse_extref_reference_pairs(bytes)
                .into_iter()
                .enumerate()
        {
            let record_key = record
                .id
                .split_once('#')
                .map_or(record.id.as_str(), |(_, key)| key);
            let id = format!("nx:external-reference:tail-reference-pair#{record_key}-{ordinal}");
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed(
                    "NX native external reference tail pair ordinal overflows u32",
                )
            })?;
            let source_offset = source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                .ok_or_else(|| {
                    CodecError::malformed("NX native external reference tail pair offset overflows")
                })?;
            out.push(ExternalReferenceTailReferencePair {
                id,
                handle_set_record: record.id.clone(),
                ordinal,
                persistent_handle,
                tagged_reference,
                source_offset,
            });
        }
    }
    Ok(out)
}

/// Resolve complete four-slot record lanes through same-stream string tables.
pub(super) fn external_reference_record_string_uses(
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
) -> Result<Vec<ExternalReferenceRecordStringUse>, CodecError> {
    let mut references_by_key = BTreeMap::<(&str, u32), Option<&ExternalReference>>::new();
    for reference in references {
        let key = (reference.source_entry.as_str(), reference.ordinal);
        if let Some(value) = references_by_key.get_mut(&key) {
            *value = None;
        } else {
            references_by_key.insert(key, Some(reference));
        }
    }
    let mut output = Vec::new();
    for record in records {
        if record
            .source_offset
            .checked_add(ExtrefSlot::Fourth.offset())
            .is_none()
        {
            continue;
        }
        let resolved = record.id_slots.map(|index| {
            references_by_key
                .get(&(record.source_entry.as_str(), index))
                .and_then(|reference| *reference)
        });
        let [Some(first), Some(second), Some(third), Some(fourth)] = resolved else {
            continue;
        };
        for (slot, reference) in ExtrefSlot::ALL
            .into_iter()
            .zip([first, second, third, fourth])
        {
            let record_key = record
                .id
                .split_once('#')
                .map_or(record.id.as_str(), |(_, key)| key);
            let id = format!(
                "nx:external-reference:record-string-use#{record_key}-{}",
                u8::from(slot)
            );
            let source_offset =
                record
                    .source_offset
                    .checked_add(slot.offset())
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "NX native external reference string use offset overflows",
                        )
                    })?;
            output.push(ExternalReferenceRecordStringUse {
                id,
                external_record: record.id.clone(),
                slot,
                string_index: record.id_slots[slot.index()],
                external_reference: reference.id.clone(),
                source_offset,
            });
        }
    }
    Ok(output)
}

/// Bind complete record lanes to their slot-zero name and slot-two directory.
pub(super) fn external_reference_record_children(
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
    uses: &[ExternalReferenceRecordStringUse],
) -> Vec<ExternalReferenceRecordChild> {
    let mut references_by_id = BTreeMap::<&str, Option<&ExternalReference>>::new();
    for reference in references {
        if let Some(value) = references_by_id.get_mut(reference.id.as_str()) {
            *value = None;
        } else {
            references_by_id.insert(reference.id.as_str(), Some(reference));
        }
    }
    let mut output = Vec::new();
    for record in records {
        let mut record_uses: [Option<&ExternalReferenceRecordStringUse>; 4] = [None; 4];
        let mut count = 0usize;
        let mut duplicate = false;
        for use_ in uses {
            if use_.external_record != record.id {
                continue;
            }
            count += 1;
            let slot = &mut record_uses[use_.slot.index()];
            if slot.is_some() {
                duplicate = true;
                break;
            }
            *slot = Some(use_);
        }
        let [Some(slot0), Some(slot1), Some(slot2), Some(slot3)] = record_uses else {
            continue;
        };
        if duplicate || count != 4 {
            continue;
        }
        let slot_uses = [slot0, slot1, slot2, slot3];
        let mut resolved = [None; 4];
        let mut valid = true;
        for (slot, use_) in slot_uses.into_iter().enumerate() {
            let Some(reference) = references_by_id
                .get(use_.external_reference.as_str())
                .and_then(|reference| *reference)
            else {
                valid = false;
                break;
            };
            if use_.string_index != record.id_slots[slot]
                || reference.source_entry != record.source_entry
                || reference.ordinal != use_.string_index
            {
                valid = false;
                break;
            }
            resolved[slot] = Some(reference);
        }
        if !valid {
            continue;
        }
        let (Some(name), Some(directory)) = (resolved[0], resolved[2]) else {
            continue;
        };
        let Some(suffix_start) = name.path.len().checked_sub(4) else {
            continue;
        };
        let Some(suffix) = name.path.get(suffix_start..) else {
            continue;
        };
        if !suffix.eq_ignore_ascii_case(".prt") || directory.path.is_empty() {
            continue;
        }

        output.push(ExternalReferenceRecordChild {
            id: format!("{}:child", record.id),
            external_record: record.id.clone(),
            name_reference: name.id.clone(),
            directory_reference: directory.id.clone(),
        });
    }
    output
}

/// Decode the explicit NX arrangement table.
pub(super) fn configurations(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<Configuration>, CodecError> {
    let mut matches = container
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.name == "/Root/part/arrangements");
    let Some((entry_index, entry)) = matches.next() else {
        return Ok(Vec::new());
    };
    if matches.next().is_some() {
        return Ok(Vec::new());
    }
    let Some((offset, size)) = entry.file_span() else {
        return Ok(Vec::new());
    };
    let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
        return Ok(Vec::new());
    };
    let Some(end) = start.checked_add(size) else {
        return Ok(Vec::new());
    };
    let Some(payload) = container.data.get(start..end) else {
        return Ok(Vec::new());
    };
    let Some(xml) = xml_stream_text(payload) else {
        return Ok(Vec::new());
    };
    let admitted_document = match ctx.parse_xml(xml, "decode XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(Vec::new());
        }
    };
    let document = admitted_document.document();
    let root = document.root_element();
    if root.tag_name().name() != "Arrangements" {
        return Ok(Vec::new());
    }
    let node_count = root.children().filter(roxmltree::Node::is_element).count();
    let mut names = BTreeSet::new();
    let mut active_count = 0usize;
    for node in root.children().filter(roxmltree::Node::is_element) {
        if node.tag_name().name() != "Arrangement" {
            return Ok(Vec::new());
        }
        let Some(name) = node.attribute("Name") else {
            return Ok(Vec::new());
        };
        if name.is_empty() || !names.insert(name) {
            return Ok(Vec::new());
        }
        let is_default = match node.attribute("Default") {
            Some("YES") => true,
            Some("NO") => false,
            _ => return Ok(Vec::new()),
        };
        active_count += usize::from(is_default);
    }
    if node_count == 0 || active_count > 1 {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    for (ordinal, node) in root
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
    {
        let Some(name) = node.attribute("Name") else {
            return Ok(Vec::new());
        };
        let is_default = node.attribute("Default") == Some("YES");
        let source_offset = offset
            .checked_add(cadmpeg_core::decode::u64_from_index(node.range().start))
            .ok_or_else(|| CodecError::malformed("NX arrangement source offset overflows"))?;
        output.push(Configuration {
            id: format!("nx:arrangements-{entry_index}:configuration#{ordinal}"),
            name: name.to_owned(),
            is_default,
            source_entry: entry.name.clone(),
            source_offset,
        });
    }
    Ok(output)
}

/// Join the two independently framed active-arrangement declarations.
pub(super) fn configuration_attribute_uses(
    configurations: &[Configuration],
    attributes: &[PartAttribute],
) -> Vec<ConfigurationAttributeUse> {
    let mut active = None;
    for configuration in configurations {
        if configuration.is_default && active.replace(configuration).is_some() {
            return Vec::new();
        }
    }
    let mut declaration = None;
    for attribute in attributes {
        if attribute.owner == "part"
            && attribute.title == "NX_Arrangement"
            && attribute.value_type == "StringAttributeType"
            && declaration.replace(attribute).is_some()
        {
            return Vec::new();
        }
    }
    let (Some(configuration), Some(attribute)) = (active, declaration) else {
        return Vec::new();
    };
    if configuration.name != attribute.value {
        return Vec::new();
    }
    vec![ConfigurationAttributeUse {
        id: "nx:arrangements:active-attribute-use#0".to_owned(),
        configuration: configuration.id.clone(),
        part_attribute: attribute.id.clone(),
        name: configuration.name.clone(),
    }]
}

/// Decode the typed part-attribute XML stream atomically.
pub(super) fn part_attributes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<PartAttribute>, CodecError> {
    let mut matches = container
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.name == "/Root/part/attrs");
    let Some((entry_index, entry)) = matches.next() else {
        return Ok(Vec::new());
    };
    if matches.next().is_some() {
        return Ok(Vec::new());
    }
    let Some((offset, size)) = entry.file_span() else {
        return Ok(Vec::new());
    };
    let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
        return Ok(Vec::new());
    };
    let Some(end) = start.checked_add(size) else {
        return Ok(Vec::new());
    };
    let Some(payload) = container.data.get(start..end) else {
        return Ok(Vec::new());
    };
    Ok(parse_part_attributes(ctx, payload, entry_index, &entry.name, offset)?.unwrap_or_default())
}

fn parse_part_attributes(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
) -> Result<Option<Vec<PartAttribute>>, CodecError> {
    let Some(xml) = xml_stream_text(payload) else {
        return Ok(None);
    };
    let admitted_document = match ctx.parse_xml(xml, "decode XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(None);
        }
    };
    let document = admitted_document.document();
    let root = document.root_element();
    let Some(version) = root
        .attribute("version")
        .and_then(|version| version.parse::<u32>().ok())
    else {
        return Ok(None);
    };
    if root.tag_name().name() != "UgAttributes" || version < 4 {
        return Ok(None);
    }
    for node in root.children().filter(roxmltree::Node::is_element) {
        if node.tag_name().name() != "Attribute"
            || node.attribute("owner").is_none()
            || node
                .attribute("utf8title")
                .or_else(|| node.attribute("title"))
                .is_none()
            || node
                .attribute("utf8value")
                .or_else(|| node.attribute("value"))
                .is_none()
            || node.attribute("type").is_none()
            || !matches!(node.attribute("pdmBased"), Some("true" | "false"))
            || node
                .attribute("version")
                .and_then(|version| version.parse::<u32>().ok())
                .is_none()
        {
            return Ok(None);
        }
    }
    let mut output = Vec::new();
    for (ordinal, node) in root
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
    {
        let Some((owner, title, value, value_type, pdm_based, version)) = (|| {
            Some((
                node.attribute("owner")?,
                node.attribute("utf8title")
                    .or_else(|| node.attribute("title"))?,
                node.attribute("utf8value")
                    .or_else(|| node.attribute("value"))?,
                node.attribute("type")?,
                node.attribute("pdmBased")? == "true",
                node.attribute("version")?.parse::<u32>().ok()?,
            ))
        })() else {
            return Ok(None);
        };
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(node.range().start))
            .ok_or_else(|| CodecError::malformed("NX part attribute source offset overflows"))?;
        output.push(PartAttribute {
            id: format!("nx:part-attributes-{entry_index}:attribute#{ordinal}"),
            owner: owner.to_owned(),
            title: title.to_owned(),
            value: value.to_owned(),
            value_type: value_type.to_owned(),
            pdm_based,
            version,
            source_entry: source_entry.to_owned(),
            source_offset,
        });
    }
    Ok(Some(output))
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

#[derive(Clone, Copy)]
enum RegistryKind {
    Class,
    Field,
}

struct RegistryDefinition {
    id: String,
    name: String,
    ordinal: u32,
    trailing_code: u8,
    registry_suffix: Vec<u8>,
    section_offset: u64,
    source_entry: String,
    source_offset: u64,
}

/// Merge both section forms with framed definitions taking precedence.
fn registry_definitions<T>(
    container: &Container,
    kind: RegistryKind,
    project: impl Fn(RegistryDefinition) -> T,
) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let framed_sections = container.om_sections()?;
    let indexed_sections = container.indexed_om_sections()?;
    let framed = framed_sections
        .into_iter()
        .map(|(entry, section)| (entry, section.offset, section.types, section.fields, true));
    let indexed = indexed_sections.into_iter().map(|(entry, section)| {
        (
            entry,
            section.base_offset(),
            section.types,
            section.fields,
            false,
        )
    });
    let mut definitions = BTreeMap::new();
    for (entry, section_offset, types, fields, replace) in framed.chain(indexed) {
        let entry_index = entry.index();
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let (marker, count) = match kind {
            RegistryKind::Class => (":class#", types.len()),
            RegistryKind::Field => (":field#", fields.len()),
        };
        for ordinal in 0..count {
            let (offset, name, tail) = match kind {
                RegistryKind::Class => {
                    let declaration = &types[ordinal];
                    (
                        declaration.offset,
                        declaration.name,
                        declaration.registry_tail,
                    )
                }
                RegistryKind::Field => {
                    let declaration = &fields[ordinal];
                    (
                        declaration.offset,
                        declaration.name,
                        declaration.registry_tail,
                    )
                }
            };
            let key = (entry_index, offset);
            if !replace && definitions.contains_key(&key) {
                continue;
            }
            let Some((&trailing_code, registry_suffix)) = tail.split_first() else {
                continue;
            };
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed("NX registry definition ordinal overflows u32")
            })?;
            let section_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(section_offset))
                .ok_or_else(|| CodecError::malformed("NX registry section offset overflows"))?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                .ok_or_else(|| CodecError::malformed("NX registry definition offset overflows"))?;
            definitions.insert(
                key,
                project(RegistryDefinition {
                    id: format!("nx:om-entry-{entry_index}{marker}{offset}"),
                    name: name.to_owned(),
                    ordinal,
                    trailing_code,
                    registry_suffix: registry_suffix.to_vec(),
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset,
                }),
            );
        }
    }
    Ok(definitions.into_values().collect())
}

/// Decode class definitions from every framed OM section.
pub(super) fn class_definitions(container: &Container) -> Result<Vec<ClassDefinition>, CodecError> {
    registry_definitions(container, RegistryKind::Class, |d| ClassDefinition {
        id: d.id,
        name: d.name,
        ordinal: d.ordinal,
        trailing_code: d.trailing_code,
        registry_suffix: (d.registry_suffix).into(),
        section_offset: d.section_offset,
        source_entry: d.source_entry,
        source_offset: d.source_offset,
    })
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
pub(super) fn field_definitions(container: &Container) -> Result<Vec<FieldDefinition>, CodecError> {
    registry_definitions(container, RegistryKind::Field, |d| FieldDefinition {
        id: d.id,
        name: d.name,
        ordinal: d.ordinal,
        trailing_code: d.trailing_code,
        registry_suffix: (d.registry_suffix).into(),
        section_offset: d.section_offset,
        source_entry: d.source_entry,
        source_offset: d.source_offset,
    })
}

/// Insert one distinct graph relation.
fn add_object_record_relation(
    relations: &mut BTreeMap<usize, Vec<usize>>,
    source: usize,
    target: usize,
) {
    let related = relations.entry(source).or_default();
    if !related.contains(&target) {
        related.push(target);
    }
}

fn object_record_relation_ids(
    relations: Option<&Vec<usize>>,
    section_ordinal: usize,
) -> Vec<String> {
    relations
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|ordinal| format!("nx:om-record-directory-{section_ordinal}:entry#{ordinal}"))
        .collect()
}

/// Catalog every externally bounded NX OM entity record.
pub(super) fn object_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ObjectRecord>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(section.base_offset()))
            .ok_or_else(|| CodecError::malformed("NX object record section offset overflows"))?;
        let record_bytes = records
            .iter()
            .map(|record| record.bytes)
            .collect::<Vec<_>>();
        let stable_identities = stable_object_record_identities(&entry.name, &record_bytes);
        let mut dependencies = BTreeMap::<usize, Vec<usize>>::new();
        let mut dependents = BTreeMap::<usize, Vec<usize>>::new();
        for (source, record) in records.iter().enumerate() {
            for reference in record.references(records.len()) {
                let RecordReference::RecordOrdinal16 { ordinal, .. } = reference.value else {
                    continue;
                };
                let target = usize::from(ordinal);
                add_object_record_relation(&mut dependencies, source, target);
                add_object_record_relation(&mut dependents, target, source);
            }
        }
        for (record_ordinal, (record, stable_identity)) in
            records.iter().zip(stable_identities).enumerate()
        {
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
                .ok_or_else(|| CodecError::malformed("NX object record source offset overflows"))?;
            let object_id_offset =
                entry_offset
                    .checked_add(record.object_id.1)
                    .ok_or_else(|| {
                        CodecError::malformed("NX object record object ID offset overflows")
                    })?;
            let section_ordinal_u32 = u32::try_from(section_ordinal).map_err(|_| {
                CodecError::malformed("NX object record section ordinal overflows u32")
            })?;
            let record_ordinal_u32 = u32::try_from(record_ordinal)
                .map_err(|_| CodecError::malformed("NX object record ordinal overflows u32"))?;

            output.push(ObjectRecord {
                id: format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                object_id: (record.object_id.0, object_id_offset),
                section_ordinal: section_ordinal_u32,
                record_ordinal: record_ordinal_u32,
                section_offset,
                byte_len: cadmpeg_core::decode::u64_from_index(record.bytes.len()),
                sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                    ctx,
                    record.bytes,
                    "NX retained record digest",
                )?,
                stable_identity,
                dependencies: object_record_relation_ids(
                    dependencies.get(&record_ordinal),
                    section_ordinal,
                ),
                dependents: object_record_relation_ids(
                    dependents.get(&record_ordinal),
                    section_ordinal,
                ),
                source_entry: entry.name.clone(),
                source_offset,
            });
        }
    }

    let mut identity_counts = BTreeMap::<(&str, &str), usize>::new();
    for record in &output {
        let Some(identity) = record.stable_identity.as_deref() else {
            continue;
        };
        *identity_counts
            .entry((record.source_entry.as_str(), identity))
            .or_default() += 1;
    }
    let unique_flags = output
        .iter()
        .map(|record| {
            record.stable_identity.as_deref().is_some_and(|identity| {
                identity_counts.get(&(record.source_entry.as_str(), identity)) == Some(&1)
            })
        })
        .collect::<Vec<_>>();
    drop(identity_counts);
    for (record, unique) in output.iter_mut().zip(unique_flags) {
        if !unique {
            record.stable_identity = None;
        }
    }
    Ok(output)
}

/// Retain the complete counted `RMFastLoad` active-object membership table.
pub(super) fn rmfastload_object_id_table(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Option<(RmFastLoadObjectIdTable, Vec<RmFastLoadObjectId>)>, CodecError> {
    let Some((entry, table)) = container.rmfastload_object_id_table() else {
        return Ok(None);
    };
    let entry_offset = entry
        .file_span()
        .ok_or_else(|| CodecError::Malformed("FastLoad table has no owning file span".into()))?
        .0;
    let values = table.object_ids.as_slice();
    let count = values.len();
    let count_u64 = u64::try_from(count)
        .map_err(|_| CodecError::NotImplemented("FastLoad object ID count exceeds u64".into()))?;
    let mut counts = HashMap::<u32, usize>::new();
    for value in values {
        *counts.entry(*value).or_default() += 1;
    }
    ctx.charge_entities(count_u64 + 1, "admit NX FastLoad native entities")?;
    let table_id = "nx:rmfastload:object-id-table#0".to_owned();
    let mut object_ids = Vec::new();

    for (ordinal, value) in values.iter().enumerate() {
        let source_offset = entry_offset
            .checked_add(u64::try_from(table.member_offset(ordinal)).map_err(|_| {
                CodecError::NotImplemented("FastLoad member offset exceeds u64".into())
            })?)
            .ok_or_else(|| {
                CodecError::Malformed("FastLoad member source offset overflows".into())
            })?;
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            CodecError::NotImplemented("FastLoad native ordinal exceeds u32".into())
        })?;
        object_ids.push(RmFastLoadObjectId {
            id: format!("nx:rmfastload:object-id#{ordinal:010}"),
            table: table_id.clone(),
            ordinal,
            value: *value,
            stable_identity: None,
            source_offset,
        });
    }
    assign_rmfastload_object_id_identities(&mut object_ids, &counts);
    let member_ids = object_ids
        .iter()
        .map(|object_id| object_id.id.clone())
        .collect();
    let native_table = RmFastLoadObjectIdTable {
        id: table_id,
        members: ObjectIdMembers::new(member_ids)
            .map_err(|message| CodecError::Malformed(message.into()))?,
        source_entry: entry.name.clone(),
        registry_source_offset: entry_offset
            .checked_add(u64::try_from(table.registry_offset).map_err(|_| {
                CodecError::NotImplemented("FastLoad registry offset exceeds u64".into())
            })?)
            .ok_or_else(|| {
                CodecError::Malformed("FastLoad registry source offset overflows".into())
            })?,
        source_offset: entry_offset
            .checked_add(u64::try_from(table.count_offset).map_err(|_| {
                CodecError::NotImplemented("FastLoad count offset exceeds u64".into())
            })?)
            .ok_or_else(|| {
                CodecError::Malformed("FastLoad count source offset overflows".into())
            })?,
    };
    Ok(Some((native_table, object_ids)))
}

/// Give a value-backed identity only to a unique member of its table.
fn assign_rmfastload_object_id_identities(
    entries: &mut [RmFastLoadObjectId],
    counts: &HashMap<u32, usize>,
) {
    for entry in entries {
        entry.stable_identity = (counts.get(&entry.value) == Some(&1))
            .then(|| format!("{}:value#{}", entry.table, entry.value));
    }
}

/// Catalog every externally bounded block in offset-only NX OM storage.
pub(super) fn data_blocks(
    container: &Container,
) -> Result<Vec<DataBlock>, cadmpeg_core::CodecError> {
    let sections = container.indexed_om_sections()?;
    let mut identity_counts = BTreeMap::<[u8; 32], usize>::new();
    for (entry, section) in &sections {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        for (role, block) in std::iter::once((DataBlockRole::Control, control))
            .chain(records.iter().map(|record| (DataBlockRole::Column, record)))
        {
            *identity_counts
                .entry(data_block_digest(&entry.name, role, block.bytes))
                .or_default() += 1;
        }
    }

    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in sections.iter().enumerate() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(section.base_offset()))
            .ok_or_else(|| CodecError::malformed("NX data block section offset overflows"))?;
        for (block_ordinal, (role, block)) in std::iter::once((DataBlockRole::Control, control))
            .chain(records.iter().map(|record| (DataBlockRole::Column, record)))
            .enumerate()
        {
            let stable_digest = data_block_digest(&entry.name, role, block.bytes);
            let stable_identity = (identity_counts.get(&stable_digest) == Some(&1))
                .then(|| data_block_hex(&stable_digest, "nx:om:data-block:"));

            let sha256 = cadmpeg_ir::hash::digest::Sha256Digest::digest(block.bytes);

            output.push(DataBlock {
                id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                section_ordinal: u32::try_from(section_ordinal).map_err(|_| {
                    CodecError::malformed("NX data block section ordinal overflows u32")
                })?,
                block_ordinal: u32::try_from(block_ordinal)
                    .map_err(|_| CodecError::malformed("NX data block ordinal overflows u32"))?,
                role,
                section_offset,
                byte_len: cadmpeg_core::decode::u64_from_index(block.bytes.len()),
                sha256,
                stable_identity,
                source_entry: entry.name.clone(),
                source_offset: entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(block.offset))
                    .ok_or_else(|| {
                        CodecError::malformed("NX data block source offset overflows")
                    })?,
            });
        }
    }
    Ok(output)
}

/// Classify every admitted complete offset-only store control lane.
pub(super) fn data_block_control_forms(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockControlForm>, CodecError> {
    let mut forms = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(form) = crate::om::offset_store_control_form(
            control.bytes,
            records.first().map(|record| record.bytes),
        ) else {
            continue;
        };
        let kind = match form {
            crate::om::OffsetStoreControlForm::ZeroPrefixed { values } => {
                let Some(value_count) = u32::try_from(values.len())
                    .ok()
                    .and_then(std::num::NonZeroU32::new)
                else {
                    continue;
                };
                DataBlockControlFormKind::ZeroPrefixed { value_count }
            }
            crate::om::OffsetStoreControlForm::ProductAnchored {
                leading_value,
                values,
            } => {
                let Some(value_count) = u32::try_from(values.len())
                    .ok()
                    .and_then(std::num::NonZeroU32::new)
                else {
                    continue;
                };
                let Some(byte_len) = std::num::NonZeroU64::new(
                    cadmpeg_core::decode::u64_from_index(control.bytes.len()),
                ) else {
                    continue;
                };
                DataBlockControlFormKind::ProductAnchored {
                    leading: leading_value,
                    value_count,
                    byte_len,
                }
            }
        };
        let id = format!("nx:om-data-block-control-forms:form#{section_ordinal}");
        let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
        let source_offset = entry
            .file_span()
            .map_or(0, |(offset, _)| offset)
            .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
            .ok_or_else(|| CodecError::malformed("NX control form source offset overflows"))?;
        ctx.charge_entities(1, "NX data block control form")?;

        forms.push(DataBlockControlForm {
            id,
            data_block,
            kind,
            source_offset,
        });
    }
    Ok(forms)
}

/// Decode complete zero-prefixed control arrays from offset-only OM stores.
pub(super) fn data_block_control_values(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockControlValue>, CodecError> {
    let mut rows = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(crate::om::OffsetStoreControlForm::ZeroPrefixed { values }) =
            crate::om::offset_store_control_form(
                control.bytes,
                records.first().map(|record| record.bytes),
            )
        else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
        for (ordinal, value) in values.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed("NX data block control value ordinal overflows u32")
            })?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
                .and_then(|offset| offset.checked_add(u64::from(ordinal).checked_mul(4)?))
                .ok_or_else(|| {
                    CodecError::malformed("NX data block control value source offset overflows")
                })?;
            let id = format!("nx:om-data-block-control-values-{section_ordinal}:value#{ordinal}");
            let block_reference = data_block.clone();
            ctx.charge_entities(1, "NX data block control value")?;

            rows.push(DataBlockControlValue {
                id,
                data_block: block_reference,
                ordinal,
                value,
                source_offset,
            });
        }
    }
    Ok(rows)
}

/// Resolve each atomic leading control lane through its store-local class registry.
pub(super) fn data_block_control_class_references(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockControlClassReference>, CodecError> {
    let framed_sections = container.om_sections()?;
    let indexed_sections = container.indexed_om_sections()?;
    let mut rows = Vec::new();
    for (section_ordinal, (entry, section)) in indexed_sections.iter().enumerate() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        if !matches!(
            crate::om::offset_store_control_form(
                control.bytes,
                records.first().map(|record| record.bytes),
            ),
            Some(crate::om::OffsetStoreControlForm::ZeroPrefixed { .. })
        ) {
            continue;
        }
        let mut registry = BTreeMap::new();
        for definition in framed_sections
            .iter()
            .filter(|(candidate, _)| candidate.index() == entry.index())
            .flat_map(|(_, section)| section.types.iter())
            .chain(
                indexed_sections
                    .iter()
                    .filter(|(candidate, _)| candidate.index() == entry.index())
                    .flat_map(|(_, section)| section.types.iter()),
            )
        {
            registry.entry(definition.offset).or_insert(definition);
        }
        let Some(ordinals) = crate::om::offset_store_control_class_ordinals(ctx, control.bytes)?
        else {
            continue;
        };
        let entry_index = entry.index();
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, class_ordinal) in ordinals.into_iter().enumerate() {
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("NX control class ordinal overflows u32"))?;

            let definition = usize::try_from(class_ordinal)
                .ok()
                .and_then(|ordinal| registry.values().nth(ordinal));

            let class = definition.map(|definition| DataBlockControlClassRef {
                definition: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                name: definition.name.to_owned(),
            });
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
                .and_then(|offset| offset.checked_add(u64::from(ordinal_u32).checked_mul(4)?))
                .ok_or_else(|| CodecError::malformed("NX control class source offset overflows"))?;
            rows.push(DataBlockControlClassReference {
                id: format!("nx:om-data-block-control-class-references-{section_ordinal}:class#{ordinal_u32}"),
                data_block: format!("nx:om-data-blocks-{section_ordinal}:block#0"),
                ordinal: ordinal_u32,
                class_ordinal,
                class,
                source_offset,
            });
        }
    }
    Ok(rows)
}

/// Decode aligned index arrays preceding a unique control-lane product anchor.
pub(super) fn data_block_control_index_values(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockControlIndexValue>, CodecError> {
    let mut rows = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(crate::om::OffsetStoreControlForm::ProductAnchored {
            leading_value,
            values,
        }) = crate::om::offset_store_control_form(
            control.bytes,
            records.first().map(|record| record.bytes),
        )
        else {
            continue;
        };
        let leading_value_width = leading_value.map_or(0, ControlLeadingValue::width);
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
        let block_count = records.len() + 1;
        for (ordinal, value) in values.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed("NX control index value ordinal overflows u32")
            })?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
                .and_then(|offset| offset.checked_add(u64::from(leading_value_width)))
                .and_then(|offset| offset.checked_add(u64::from(ordinal).checked_mul(4)?))
                .ok_or_else(|| {
                    CodecError::malformed("NX control index value source offset overflows")
                })?;
            let id =
                format!("nx:om-data-block-control-index-values-{section_ordinal}:value#{ordinal}");
            let block_reference = data_block.clone();
            let target_data_block = if usize::try_from(value)
                .ok()
                .is_some_and(|target| target < block_count)
            {
                Some(format!("nx:om-data-blocks-{section_ordinal}:block#{value}"))
            } else {
                None
            };
            ctx.charge_entities(1, "NX data block control index value")?;

            rows.push(DataBlockControlIndexValue {
                id,
                data_block: block_reference,
                ordinal,
                value,
                target_data_block,
                source_offset,
            });
        }
    }
    Ok(rows)
}

fn control_index_data_block(
    section_ordinal: usize,
    block_count: usize,
    value: u32,
) -> Option<String> {
    usize::try_from(value)
        .ok()
        .filter(|ordinal| *ordinal < block_count)
        .map(|ordinal| format!("nx:om-data-blocks-{section_ordinal}:block#{ordinal}"))
}

fn column_storage_block_at(
    records: &[crate::om::EntityRecord<'_>],
    offset: usize,
) -> Option<(usize, u32)> {
    records.iter().enumerate().find_map(|(ordinal, record)| {
        let block_offset = offset.checked_sub(record.offset)?;
        if block_offset >= record.bytes.len() {
            return None;
        }
        let block_offset = u32::try_from(block_offset).ok()?;
        Some((ordinal.checked_add(1)?, block_offset))
    })
}

/// Decode persistent-handle and tagged-28 occurrences in bounded control blocks.
pub(super) fn data_block_control_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockControlReference>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some((control, _, _)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
        for (ordinal, reference) in crate::om::references(control.bytes, control.offset)
            .into_iter()
            .enumerate()
        {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("NX control reference ordinal overflows u32"))?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                .ok_or_else(|| {
                    CodecError::malformed("NX control reference source offset overflows")
                })?;
            let id = format!(
                "nx:om-data-block-control-references-{section_ordinal}:reference#{}",
                reference.offset
            );
            let block_reference = data_block.clone();
            ctx.charge_entities(1, "NX data block control reference")?;

            output.push(DataBlockControlReference {
                id,
                data_block: block_reference,
                ordinal,
                reference: reference.value,
                source_offset,
            });
        }
    }
    Ok(output)
}

/// Join maximal two-token adjacent persistent-handle runs atomically.
pub(super) fn data_block_control_handle_pairs(
    ctx: &DecodeContext<'_>,
    references: &[DataBlockControlReference],
) -> Result<Vec<DataBlockControlHandlePair>, CodecError> {
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
        block_references.sort_by_key(|(left, _)| left.source_offset);
        let mut at = 0;
        while at < block_references.len() {
            let start = at;
            while at
                .checked_add(1)
                .and_then(|next| block_references.get(next))
                .is_some_and(|next| {
                    block_references[at].0.source_offset.checked_add(5)
                        == Some(next.0.source_offset)
                })
            {
                at += 1;
            }
            let run = &block_references[start..=at];
            if let [(first, first_handle), (second, second_handle)] = run {
                let id = format!(
                    "nx:om-data-block-control:handle-pair#{}",
                    first.source_offset
                );
                let data_block = data_block.to_owned();
                let first_reference = first.id.clone();
                let second_reference = second.id.clone();
                ctx.charge_entities(1, "NX control handle pair")?;

                pairs.push(DataBlockControlHandlePair {
                    id,
                    data_block,
                    first_reference,
                    second_reference,
                    first_handle: *first_handle,
                    second_handle: *second_handle,
                    source_offset: first.source_offset,
                });
            }
            at += 1;
        }
    }
    Ok(pairs)
}

/// Decode framed object references from offset-only OM data blocks.
pub(super) fn data_block_references(
    container: &Container,
    object_records: &[ObjectRecord],
    expression_declarations: &[ExpressionDeclaration],
) -> Result<Vec<DataBlockReference>, cadmpeg_core::CodecError> {
    let mut target_records = BTreeMap::<(&str, u32), Vec<&str>>::new();
    for record in object_records {
        let (object_id, _) = record.object_id;
        target_records
            .entry((record.source_entry.as_str(), object_id))
            .or_default()
            .push(record.id.as_str());
    }
    let mut declarations = BTreeMap::<(&str, u32), Vec<&str>>::new();
    for declaration in expression_declarations {
        declarations
            .entry((declaration.source_entry.as_str(), declaration.object_id))
            .or_default()
            .push(declaration.id.as_str());
    }
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (block_ordinal, block) in std::iter::once(control).chain(records).enumerate() {
            for (ordinal, reference) in crate::om::data_block_object_references(block.bytes)
                .into_iter()
                .enumerate()
            {
                let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                    CodecError::malformed("NX data block reference ordinal overflows u32")
                })?;
                let key = (entry.name.as_str(), reference.object_index.value());
                let unique = |candidates: Option<&Vec<&str>>| match candidates.map(Vec::as_slice) {
                    Some([target]) => Some((*target).to_owned()),
                    _ => None,
                };
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(block.offset))
                    .and_then(|offset| {
                        offset.checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                    })
                    .ok_or_else(|| {
                        CodecError::malformed("NX data block reference source offset overflows")
                    })?;

                output.push(DataBlockReference {
                    id: format!(
                        "nx:om-data-block-references-{section_ordinal}-{block_ordinal}:reference#{ordinal}"
                    ),
                    data_block: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    ordinal: ordinal_u32,
                    object: reference.object_index,
                    target_record: unique(target_records.get(&key)),
                    target_expression_declaration: unique(declarations.get(&key)),
                    source_offset,
                });
            }
        }
    }
    Ok(output)
}

/// Decode complete part-local color tables from class-declaring offset stores.
pub(super) fn part_color_tables(
    container: &Container,
) -> Result<(Vec<PartColorTable>, Vec<PartColorDefinition>), CodecError> {
    const CLASS_NAME: &str = "UGS::COLOR_table";
    let mut tables = Vec::new();
    let mut definitions = Vec::new();

    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
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
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_base = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(storage_offset))
            .ok_or_else(|| CodecError::malformed("NX part color source base overflows"))?;
        let table_id = format!("nx:part-color-tables:table#{section_ordinal}");
        let mut parsed_definitions = Vec::new();
        let mut definition_ids = Vec::new();
        for color_index in PaletteIndex::all() {
            let definition = &table.definitions[usize::from(color_index.value()) - 1];
            let id = format!(
                "nx:part-color-definitions-{section_ordinal}:color#{}",
                color_index.value()
            );
            definition_ids.push(id.clone());
            let [a, b, c] = definition.components.map(|(component, offset)| {
                source_base
                    .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                    .map(|offset| (component, offset))
                    .ok_or_else(|| {
                        CodecError::malformed("NX part color component offset overflows")
                    })
            });
            let components = [a?, b?, c?];
            let source_offset = source_base
                .checked_add(cadmpeg_core::decode::u64_from_index(definition.offset))
                .ok_or_else(|| {
                    CodecError::malformed("NX part color definition offset overflows")
                })?;
            parsed_definitions.push(PartColorDefinition {
                id,
                color_table: table_id.clone(),
                color_index,
                name: definition.name.to_owned(),
                components,
                source_offset,
            });
        }
        let definition_ids: [String; PALETTE_SIZE] = definition_ids.try_into().map_err(|_| {
            CodecError::malformed(
                "NX part color table definition count differs from the palette size",
            )
        })?;
        let [a, b, c] = table.background.map(|(component, offset)| {
            source_base
                .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                .map(|offset| (component, offset))
                .ok_or_else(|| CodecError::malformed("NX part color background offset overflows"))
        });
        let background = [a?, b?, c?];
        let source_offset = source_base
            .checked_add(cadmpeg_core::decode::u64_from_index(table.offset))
            .ok_or_else(|| CodecError::malformed("NX part color table offset overflows"))?;

        definitions.append(&mut parsed_definitions);
        tables.push(PartColorTable {
            id: table_id,
            class_definition: format!("nx:om-entry-{entry_index}:class#{}", class.offset),
            background,
            definitions: definition_ids,
            source_entry: entry.name.clone(),
            source_offset,
        });
    }

    Ok((tables, definitions))
}

fn rmfastload_target_object_id(object_ids: &[RmFastLoadObjectId], target: u32) -> Option<String> {
    let object_id = object_ids.get(usize::try_from(target).ok()?)?;
    Some(object_id.id.clone())
}

/// Resolve complete composite column-index tables atomically by section.
pub(super) fn data_block_column_index_tables(
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
    let mut output = Vec::new();
    for (section_ordinal, linked) in linked_by_section {
        let Some(targets) = targets_by_section.remove(&section_ordinal) else {
            continue;
        };
        let Some((opening, suffix)) = linked.split_first() else {
            continue;
        };
        let Some((last_target, target_prefix)) = targets.split_last() else {
            continue;
        };
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
            continue;
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
        );
        let mut previous: Option<(u32, u64)> = None;
        let valid_order = ordered.into_iter().all(|current| {
            let valid = previous.is_none_or(|previous| {
                previous.0.checked_sub(1) == Some(current.0) && previous.1 < current.1
            });
            previous = Some(current);
            valid
        });
        if !valid_order
            || linked
                .iter()
                .any(|row| row.source_entry != opening.source_entry)
            || targets
                .iter()
                .any(|row| row.source_entry != opening.source_entry)
        {
            continue;
        }
        let target_ids = targets.iter().map(|row| row.id.clone()).collect();
        let suffix_ids = suffix.iter().map(|row| row.id.clone()).collect();
        let Ok(rows) = ColumnIndexRows::new(
            opening.frame.target_index().atom.value(),
            target_ids,
            suffix_ids,
        ) else {
            continue;
        };

        output.push(DataBlockColumnIndexTable {
            id: format!("nx:om-data-block-column-index-tables:table#{section_ordinal}"),
            section_ordinal,
            opening_linked_row: opening.id.clone(),
            rows,
            source_entry: opening.source_entry.clone(),
            source_offset: opening.frame.offset(),
        });
    }
    output
}

/// Decode one product/version header from each indexed NX OM store.
pub(super) fn store_headers(
    container: &Container,
) -> Result<Vec<StoreHeader>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let candidate = match &section.store {
            IndexedStore::Fixed { records } => records.iter().find_map(|record| {
                crate::om::store_version(record.bytes, record.offset)
                    .map(|version| (Some(record.object_id.0), version))
            }),
            IndexedStore::OffsetOnly {
                control, records, ..
            } => std::iter::once(control)
                .chain(records.iter())
                .find_map(|record| {
                    crate::om::store_version(record.bytes, record.offset)
                        .map(|version| (None, version))
                }),
        };
        let Some((object_id, version)) = candidate else {
            continue;
        };
        let section_ordinal_u32 = u32::try_from(section_ordinal)
            .map_err(|_| CodecError::malformed("NX store header section ordinal overflows u32"))?;
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(version.offset))
            .ok_or_else(|| CodecError::malformed("NX store header source offset overflows"))?;

        let header = OffsetStoreHeader {
            id: format!("nx:om-store-headers:store#{section_ordinal}"),
            section_ordinal: section_ordinal_u32,
            version: version.value.into_owned(),
            source_entry: entry.name.clone(),
            source_offset,
        };
        output.push(match object_id {
            Some(object_id) => StoreHeader::Fixed(FixedStoreHeader { object_id, header }),
            None => StoreHeader::OffsetOnly(header),
        });
    }
    Ok(output)
}

/// Decode self-framed printable values from bounded NX OM records.
pub(super) fn string_values(
    container: &Container,
) -> Result<Vec<StringValue>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (record_ordinal, record) in records.iter().enumerate() {
            for (value_ordinal, value) in record.string_values().into_iter().enumerate() {
                let record_id =
                    format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                let value_ordinal = u32::try_from(value_ordinal)
                    .map_err(|_| CodecError::malformed("NX string value ordinal overflows u32"))?;
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                    .ok_or_else(|| {
                        CodecError::malformed("NX string value source offset overflows")
                    })?;
                let text = value.value.as_str().to_owned();
                let text = PrintableString::new(text)
                    .map_err(|_| CodecError::malformed("NX string value is not printable"))?;

                output.push(StringValue {
                    id: format!(
                        "nx:om-string-values-{section_ordinal}-{record_ordinal}:value#{}",
                        value.offset
                    ),
                    record: record_id,
                    object_id: record.object_id.0,
                    ordinal: value_ordinal,
                    value: text,
                    source_entry: entry.name.clone(),
                    source_offset,
                });
            }
        }
    }
    Ok(output)
}

/// Decode ordered tagged references from bounded NX OM records.
pub(super) fn object_references(
    container: &Container,
) -> Result<Vec<ObjectReference>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (record_ordinal, record) in records.iter().enumerate() {
            for (reference_ordinal, reference) in
                record.references(records.len()).into_iter().enumerate()
            {
                let record_id =
                    format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                let reference_ordinal = u32::try_from(reference_ordinal).map_err(|_| {
                    CodecError::malformed("NX object reference ordinal overflows u32")
                })?;
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                    .ok_or_else(|| {
                        CodecError::malformed("NX object reference source offset overflows")
                    })?;
                let reference_value = match reference.value {
                    RecordReference::Direct(value) => RecordReference::Direct(value),
                    RecordReference::RecordOrdinal16 { ordinal, .. } => {
                        RecordReference::RecordOrdinal16 {
                            ordinal,
                            target: format!(
                                "nx:om-record-directory-{section_ordinal}:entry#{ordinal}"
                            ),
                        }
                    }
                };

                output.push(ObjectReference {
                    id: format!(
                        "nx:om-references-{section_ordinal}-{record_ordinal}:reference#{}",
                        reference.offset
                    ),
                    record: record_id,
                    object_id: record.object_id.0,
                    ordinal: reference_ordinal,
                    reference: reference_value,
                    source_entry: entry.name.clone(),
                    source_offset,
                });
            }
        }
    }
    Ok(output)
}

/// Join maximal two-token adjacent persistent-handle runs within object records.
pub(super) fn object_record_handle_pairs(
    references: &[ObjectReference],
) -> Vec<ObjectRecordHandlePair> {
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
        record_references.sort_by_key(|(left, _)| left.source_offset);
        let mut at = 0;
        while at < record_references.len() {
            let start = at;
            while record_references.get(at + 1).is_some_and(|next| {
                record_references[at].0.source_offset.checked_add(5) == Some(next.0.source_offset)
            }) {
                at += 1;
            }
            let run = &record_references[start..=at];
            if let [(first, first_handle), (second, second_handle)] = run {
                pairs.push(ObjectRecordHandlePair {
                    id: format!("nx:om-object-record:handle-pair#{}", first.source_offset),
                    record: record.to_owned(),
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
pub(super) fn persistent_handles(
    references: &[ObjectReference],
    control_references: &[DataBlockControlReference],
    external: &[ExternalReferenceRecord],
    external_tail_pairs: &[ExternalReferenceTailReferencePair],
) -> Result<Vec<PersistentHandle>, CodecError> {
    #[derive(Default)]
    struct Group<'a> {
        records: Vec<&'a str>,
        occurrence_count: u32,
        external_records: Vec<&'a str>,
        data_blocks: Vec<&'a str>,
        external_occurrence_count: u32,
    }

    let mut groups = BTreeMap::<u32, Group<'_>>::new();
    for reference in references {
        let RecordReference::Direct(DirectReference::PersistentHandle(handle)) =
            reference.reference
        else {
            continue;
        };
        let group = groups.entry(handle).or_default();
        group.occurrence_count = group.occurrence_count.checked_add(1).ok_or_else(|| {
            CodecError::malformed("NX persistent handle occurrence count overflows u32")
        })?;
        if group.records.last().copied() != Some(reference.record.as_str())
            && !group.records.contains(&reference.record.as_str())
        {
            group.records.push(reference.record.as_str());
        }
    }
    for reference in control_references {
        let DirectReference::PersistentHandle(handle) = reference.reference else {
            continue;
        };
        let group = groups.entry(handle).or_default();
        group.occurrence_count = group.occurrence_count.checked_add(1).ok_or_else(|| {
            CodecError::malformed("NX persistent handle occurrence count overflows u32")
        })?;
        if !group.data_blocks.contains(&reference.data_block.as_str()) {
            group.data_blocks.push(reference.data_block.as_str());
        }
    }
    for record in external {
        for handle in record.handles.serialized() {
            let group = groups.entry(*handle).or_default();
            group.external_occurrence_count = group
                .external_occurrence_count
                .checked_add(1)
                .ok_or_else(|| {
                    CodecError::malformed("NX external handle occurrence count overflows u32")
                })?;
            if !group.external_records.contains(&record.id.as_str()) {
                group.external_records.push(record.id.as_str());
            }
        }
    }
    for pair in external_tail_pairs {
        let group = groups.entry(pair.persistent_handle).or_default();
        group.external_occurrence_count = group
            .external_occurrence_count
            .checked_add(1)
            .ok_or_else(|| {
                CodecError::malformed("NX external handle occurrence count overflows u32")
            })?;
        if !group
            .external_records
            .contains(&pair.handle_set_record.as_str())
        {
            group.external_records.push(pair.handle_set_record.as_str());
        }
    }
    let mut handles = Vec::new();
    for (value, group) in groups {
        handles.push(PersistentHandle {
            id: format!("nx:om-persistent-handles:handle#{value:08x}"),
            value,
            records: group.records.iter().map(|&id| id.to_owned()).collect(),
            occurrence_count: group.occurrence_count,
            data_blocks: group.data_blocks.iter().map(|&id| id.to_owned()).collect(),
            external_records: group
                .external_records
                .iter()
                .map(|&id| id.to_owned())
                .collect(),
            external_occurrence_count: group.external_occurrence_count,
        });
    }
    Ok(handles)
}

/// Decode named parameter declarations from expression-class OM records.
pub(super) fn expression_declarations(
    container: &Container,
) -> Result<Vec<ExpressionDeclaration>, CodecError> {
    let mut declarations = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections()?.into_iter().enumerate()
    {
        if !section
            .types
            .iter()
            .any(|definition| definition.name == "UGS::EXP_expression")
        {
            continue;
        }
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let Some(records) = section.as_fixed() else {
            continue;
        };
        for (record_ordinal, record) in records.iter().enumerate() {
            let Some(declaration) = crate::om::expression_declaration_name(record.bytes) else {
                continue;
            };
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
                .and_then(|offset| {
                    offset.checked_add(cadmpeg_core::decode::u64_from_index(declaration.offset))
                })
                .ok_or_else(|| CodecError::malformed("NX declaration source offset overflows"))?;
            let name = declaration.name.as_str().to_owned();
            let name = ParameterName::<String, u32>::parse(name).ok_or_else(|| {
                CodecError::malformed("NX expression declaration name is not a parameter name")
            })?;
            let literal = declaration.literal.map(str::to_owned);

            declarations.push(ExpressionDeclaration {
                id: format!(
                    "nx:om-expression-declarations-{section_ordinal}:declaration#{record_ordinal}"
                ),
                object_id: record.object_id.0,
                record: format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                name,
                literal,
                source_entry: entry.name.clone(),
                source_offset,
            });
        }
    }
    Ok(declarations)
}

/// Decode explicit numeric expressions from all indexed OM sections.
pub(super) fn expressions(
    container: &Container,
    declarations: &[ExpressionDeclaration],
) -> Result<Vec<ParameterFormula>, CodecError> {
    let mut declarations_by_name = BTreeMap::<(&str, &str), Vec<&ExpressionDeclaration>>::new();
    for declaration in declarations {
        declarations_by_name
            .entry((declaration.source_entry.as_str(), declaration.name.as_str()))
            .or_default()
            .push(declaration);
    }
    let sections = container.indexed_om_sections()?;
    let mut indexed = BTreeMap::<(&str, usize), (u32, usize, usize)>::new();
    for (section_ordinal, (entry, section)) in sections.into_iter().enumerate() {
        let Some(directory_entry) = container.entries.get(entry.index()) else {
            continue;
        };
        for (record_ordinal, expression) in section.numeric_expression_records() {
            let Some(object_id) = expression.object_id else {
                continue;
            };
            indexed.insert(
                (directory_entry.name.as_str(), expression.offset),
                (object_id, section_ordinal, record_ordinal),
            );
        }
    }
    let mut expressions = Vec::new();
    for (entry_index, entry) in container.entries.iter().enumerate() {
        let Some((entry_offset, size)) = entry.file_span() else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(entry_offset), usize::try_from(size)) else {
            continue;
        };
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        let Some(payload) = container.data.get(offset..end) else {
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
                .get(&(entry.name.as_str(), expression.offset))
                .copied();
            let declaration = declarations_by_name
                .get(&(entry.name.as_str(), expression.name.as_str()))
                .and_then(|candidates| {
                    let mut matches = candidates.iter().copied().filter(|declaration| {
                        indexed_record.is_none_or(|(_, section_ordinal, _)| {
                            declaration
                                .record
                                .split_once(":entry#")
                                .and_then(|(prefix, _)| {
                                    prefix.strip_prefix("nx:om-record-directory-")
                                })
                                .and_then(|ordinal| ordinal.parse::<usize>().ok())
                                == Some(section_ordinal)
                        })
                    });
                    let first = matches.next()?;
                    matches.next().is_none().then_some(first)
                });
            let value = expression.constant_value();
            let source_table_text =
                format!("nx:om-entry-{entry_index}:expression-table#{table_offset}");
            let Some(source_table) = cadmpeg_core::text::NonBlankString::new(source_table_text)
            else {
                continue;
            };
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(expression.offset))
                .ok_or_else(|| CodecError::malformed("NX expression source offset overflows"))?;

            expressions.push(ParameterFormula {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                owner: indexed_record.map(|(object_id, section_ordinal, record_ordinal)| {
                    ExpressionOwner {
                        object_id,
                        record: format!(
                            "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                        ),
                    }
                }),
                declaration: declaration.map(|declaration| declaration.id.clone()),
                name: ParameterName::new(expression.name.as_str().to_owned()),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Inch => ExpressionUnit::Inch,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                    crate::om::ExpressionUnit::Native(unit) => ExpressionUnit::Native(unit),
                },
                expression: expression.expression.to_owned(),
                value,
                source_entry: entry.name.clone(),
                source_table,
                source_offset,
            });
        }
    }
    evaluate_expression_graphs(&mut expressions);
    Ok(expressions)
}

fn evaluate_expression_graphs(expressions: &mut [ParameterFormula]) {
    struct Group {
        count: usize,
        value: Option<FiniteReal>,
    }

    let mut groups = BTreeMap::<(&str, &str, &ExpressionUnit), Group>::new();
    for expression in expressions.iter() {
        let key = (
            expression.source_table.as_str(),
            expression.name.as_str(),
            &expression.unit,
        );
        match groups.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Group {
                    count: 1,
                    value: expression.value,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let group = entry.get_mut();
                group.count += 1;
                group.value = None;
            }
        }
    }
    loop {
        let mut changed = false;

        for expression in expressions
            .iter()
            .filter(|expression| expression.value.is_none())
        {
            let key = (
                expression.source_table.as_str(),
                expression.name.as_str(),
                &expression.unit,
            );
            if !groups
                .get(&key)
                .is_some_and(|group| group.count == 1 && group.value.is_none())
            {
                continue;
            }
            let evaluated = evaluate_parameterized_expression(&expression.expression, |name| {
                let dependency = (expression.source_table.as_str(), name, &expression.unit);
                groups
                    .get(&dependency)
                    .filter(|group| group.count == 1)
                    .and_then(|group| group.value)
                    .map(FiniteReal::get)
            });
            if let Some(value) = evaluated {
                if let Some(group) = groups.get_mut(&key) {
                    group.value = Some(value);
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let results = expressions
        .iter()
        .map(|expression| {
            let key = (
                expression.source_table.as_str(),
                expression.name.as_str(),
                &expression.unit,
            );
            groups
                .get(&key)
                .filter(|group| group.count == 1)
                .and_then(|group| group.value)
        })
        .collect::<Vec<_>>();
    drop(groups);
    for (expression, value) in expressions.iter_mut().zip(results) {
        expression.value = value;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod rmfastload;

#[cfg(test)]
mod object_record_identity_tests {

    use crate::test_support::test_prt::prt_with_indexed_om_section;

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
        let records =
            crate::test_support::with_decode_context(|ctx| super::object_records(ctx, &container))
                .unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record.stable_identity.is_some()));
        assert_ne!(records[0].stable_identity, records[1].stable_identity);
    }

    fn stable_identities_for_test(records: &[&[u8]]) -> Vec<Option<String>> {
        super::stable_object_record_identities("/entry", records)
    }

    #[test]
    fn graph_identity_ignores_same_section_record_reordering() {
        let first: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x01, 0xa0];
        let second: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x00, 0xb0];
        let original = [first, second];

        let reordered_first: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x01, 0xb0];
        let reordered_second: &[u8] = &[0x01, 0x02, 0x90, 0x00, 0x00, 0xa0];
        let reordered = [reordered_first, reordered_second];

        let original_identities = stable_identities_for_test(&original);
        let reordered_identities = stable_identities_for_test(&reordered);
        assert_eq!(original_identities[0], reordered_identities[1]);
        assert_eq!(original_identities[1], reordered_identities[0]);

        let unrelated: &[u8] = &[0xd0];
        let with_unrelated = [original[0], original[1], unrelated];
        let with_unrelated_identities = stable_identities_for_test(&with_unrelated);
        assert_eq!(original_identities[0], with_unrelated_identities[0]);
        assert_eq!(original_identities[1], with_unrelated_identities[1]);

        let changed = [reordered_first, &[0x01, 0x02, 0x90, 0x00, 0x00, 0xc0][..]];
        let changed_identities = stable_identities_for_test(&changed);
        assert_ne!(original_identities[0], changed_identities[1]);
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_qualifier, String, "qualifier");
cadmpeg_core::named_optional_field!(deserialize_literal, String, "literal");
cadmpeg_core::named_optional_field!(deserialize_declaration, String, "declaration");
cadmpeg_core::named_optional_field!(deserialize_value, f64, "value");
cadmpeg_core::named_optional_field!(
    deserialize_registry_storage_code,
    u32,
    "registry_storage_code"
);
cadmpeg_core::named_optional_field!(deserialize_registry_base_class, u32, "registry_base_class");
cadmpeg_core::named_optional_field!(deserialize_registry_reference, u32, "registry_reference");
cadmpeg_core::named_optional_field!(
    deserialize_schema_fingerprint,
    cadmpeg_ir::native::bytes::NativeBytes<[u8; 8]>,
    "schema_fingerprint"
);
cadmpeg_core::named_optional_field!(deserialize_layout_terminal, u8, "layout_terminal");
cadmpeg_core::named_optional_field!(
    deserialize_registry_owner_class,
    u32,
    "registry_owner_class"
);
cadmpeg_core::named_optional_field!(deserialize_stable_identity, String, "stable_identity");
cadmpeg_core::named_optional_field!(deserialize_leading_value_width, u8, "leading_value_width");
cadmpeg_core::named_optional_field!(deserialize_leading_value, u32, "leading_value");
cadmpeg_core::named_optional_field!(deserialize_target_data_block, String, "target_data_block");
cadmpeg_core::named_optional_field!(deserialize_class_definition, String, "class_definition");
cadmpeg_core::named_optional_field!(deserialize_class_name, String, "class_name");
cadmpeg_core::named_optional_field!(deserialize_target_record, String, "target_record");
cadmpeg_core::named_optional_field!(
    deserialize_target_expression_declaration,
    String,
    "target_expression_declaration"
);
cadmpeg_core::named_optional_field!(deserialize_handle_set_record, String, "handle_set_record");
