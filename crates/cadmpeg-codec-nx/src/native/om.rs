// SPDX-License-Identifier: Apache-2.0
//! Object-model, data-block, expression, and external-reference extractors and record types.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

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
    pub(super) sha256: crate::native::hex::Sha256Hex,
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
        source_offset.checked_add(record.byte_len() as u64)?;
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
        self.source_offset + self.record.byte_len() as u64
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
    let links = segment_om_links(ctx, container)?;
    let sections = container.om_sections(ctx)?;
    let mut areas = Vec::new();
    for link in links {
        let Some((_, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
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
            .checked_sub(section.offset as u64)
        else {
            continue;
        };
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(header.offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX OM record area source offset", 0, 1))?;
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let prefix = "nx:om-record-areas:area#";
        let digits = header
            .offset
            .checked_ilog10()
            .map_or(1, |count| count as usize + 1);
        let id_len = prefix
            .len()
            .checked_add(section_key.len())
            .and_then(|length| length.checked_add(1))
            .and_then(|length| length.checked_add(digits))
            .ok_or_else(|| ctx.refuse_codec_limit("NX OM record area id", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(id_len),
            "NX OM record area id",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_len),
            "NX OM record area id",
        )?;
        let mut id = String::new();
        id.try_reserve_exact(id_len)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX OM record area id", 0, 1))?;
        write!(&mut id, "{prefix}{section_key}-{}", header.offset)
            .map_err(|_| ctx.refuse_codec_limit("write NX OM record area id", 0, 1))?;
        ctx.charge_collection_items(1, "NX OM record areas")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OmRecordArea>()),
            "retain NX OM record area",
        )?;
        areas
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX OM record areas", 0, 1))?;
        areas.push(OmRecordArea {
            id,
            section_link: link.id,
            schema_role: link.schema_role,
            control_words: header.control_words,
            product_version: header.product.value.try_into_owned_for_decode(ctx)?,
            byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
            sha256: crate::native::hex::Sha256Hex::digest(bytes),
            source_offset,
        });
    }
    Ok(areas)
}

/// Build a retained identity with two ten-digit minimum ordinals.
fn retained_om_padded_state_id(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    section_ordinal: usize,
    ordinal: u32,
    operation: &'static str,
) -> Result<String, CodecError> {
    let section_digits = section_ordinal
        .checked_ilog10()
        .map_or(1, |count| count as usize + 1)
        .max(10);
    let length = prefix
        .len()
        .checked_add(section_digits)
        .and_then(|length| length.checked_add(11))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(&mut id, "{prefix}{section_ordinal:010}-{ordinal:010}")
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

/// Decode complete rows from audit-trail record areas.
pub(super) fn audit_trail_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmAuditTrailRow>, cadmpeg_core::CodecError> {
    let sections = container.om_sections(ctx)?;
    let mut out = Vec::new();
    for (section_ordinal, link) in segment_om_links(ctx, container)?
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::AuditTrail)
        .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX audit section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(rows) = section.audit_trail_rows(ctx)? else {
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
            let id = retained_om_padded_state_id(
                ctx,
                "nx:audit-trail:row#",
                section_ordinal,
                ordinal,
                "NX audit trail row id",
            )?;
            let section_link = ctx.copy_retained_text(&link.id, "NX audit trail section link")?;
            let source_entry =
                ctx.copy_retained_text(&entry.name, "NX audit trail source entry")?;
            let Some(result) =
                OmAuditTrailRow::new(id, section_link, record, source_entry, source_offset)
            else {
                continue;
            };
            ctx.charge_collection_items(1, "NX audit trail rows")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OmAuditTrailRow>()),
                "retain NX audit trail row",
            )?;
            out.try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("allocate NX audit trail rows", 0, 1))?;
            out.push(result);
        }
    }
    Ok(out)
}

/// Decode exact object state-counter rows from canonical feature-history areas.
pub(super) fn operation_state_counters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmOperationStateCounter>, cadmpeg_core::CodecError> {
    let sections = container.om_sections(ctx)?;
    let mut out = Vec::new();
    for (section_ordinal, link) in crate::native::features::canonical_feature_history_links(
        ctx,
        segment_om_links(ctx, container)?,
    )?
    .into_iter()
    .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX state counter section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(map) = section.operation_state_counter_map(ctx)? else {
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
            ctx.charge_collection_items(1, "NX operation state counters")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OmOperationStateCounter>()), "retain NX operation state counter")?;
            out.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX operation state counters", 0, 1)
            })?;
            out.push(OmOperationStateCounter {
                id: retained_om_padded_state_id(
                    ctx,
                    "nx:feature-history:operation-state-counter#",
                    section_ordinal,
                    ordinal,
                    "NX operation state counter id",
                )?,
                section_link: ctx.copy_retained_text(&link.id, "NX state counter section link")?,
                ordinal,
                frame,
                source_entry: ctx.copy_retained_text(&entry.name, "NX state counter source entry")?,
            });
        }
    }
    Ok(out)
}

/// Decode anchored state-journal groups from canonical feature-history areas.
pub(super) fn operation_state_journal_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmOperationStateJournalGroup>, cadmpeg_core::CodecError> {
    let sections = container.om_sections(ctx)?;
    let mut out = Vec::new();
    for (section_ordinal, link) in crate::native::features::canonical_feature_history_links(
        ctx,
        segment_om_links(ctx, container)?,
    )?
    .into_iter()
    .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX state journal section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(groups) = section.operation_state_journal_groups(ctx)? else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, group) in groups.into_iter().enumerate() {
            let Some(ordinal) = u32::try_from(ordinal).ok() else {
                continue;
            };
            let Some(frame) = group.into_absolute(ctx, entry_offset)? else {
                continue;
            };
            ctx.charge_collection_items(1, "NX operation state journal groups")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    OmOperationStateJournalGroup,
                >()),
                "retain NX operation state journal group",
            )?;
            out.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX operation state journal groups", 0, 1)
            })?;
            out.push(OmOperationStateJournalGroup {
                id: retained_om_padded_state_id(
                    ctx,
                    "nx:feature-history:operation-state-journal-group#",
                    section_ordinal,
                    ordinal,
                    "NX operation state journal group id",
                )?,
                section_link: ctx.copy_retained_text(&link.id, "NX state journal section link")?,
                ordinal,
                frame,
                source_entry: ctx.copy_retained_text(&entry.name, "NX state journal source entry")?,
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
    let sections = container.om_sections(ctx)?;
    let mut output = Vec::new();
    for (section_ordinal, link) in crate::native::features::canonical_feature_history_links(
        ctx,
        segment_om_links(ctx, container)?,
    )?
    .into_iter()
    .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX roll-forward section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let Some(table) = section.operation_state_group_table(ctx)? else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let table_end_offset = entry_offset + table.end_offset() as u64;
        let table_footer = table.footer();
        let frames = table
            .into_groups()
            .into_iter()
            .filter_map(|group| group.into_absolute(entry_offset));
        let table = OmRollForwardStateTable::from_frames(
            ctx,
            section_ordinal,
            &link.id,
            &entry.name,
            table_footer,
            table_end_offset,
            frames,
        )?;
        ctx.charge_collection_items(1, "NX roll-forward state tables")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OmRollForwardStateTable>()),
            "retain NX roll-forward state tables",
        )?;
        output
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX roll-forward state tables", 0, 1))?;
        output.push(table);
    }
    Ok(output)
}

/// Decode standalone operation-state messages from canonical feature-history areas.
pub(super) fn operation_state_messages(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OmOperationStateMessage>, CodecError> {
    let sections = container.om_sections(ctx)?;
    let mut output = Vec::new();
    for (section_ordinal, link) in crate::native::features::canonical_feature_history_links(
        ctx,
        segment_om_links(ctx, container)?,
    )?
    .into_iter()
    .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX state message section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
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
                .ok_or_else(|| ctx.refuse_codec_limit("NX state message source offset", 0, 1))?;
            let body = message.body().into_owned(ctx)?;
            ctx.charge_collection_items(1, "NX operation state messages")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OmOperationStateMessage>()), "retain NX operation state message")?;
            output.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX operation state messages", 0, 1)
            })?;
            output.push(OmOperationStateMessage {
                id: retained_om_padded_state_id(
                    ctx,
                    "nx:feature-history:operation-state-message#",
                    section_ordinal,
                    ordinal,
                    "NX operation state message id",
                )?,
                section_link: ctx.copy_retained_text(&link.id, "NX state message section link")?,
                ordinal,
                body,
                source_entry: ctx.copy_retained_text(&entry.name, "NX state message source entry")?,
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
    let sections = container.om_sections(ctx)?;
    let mut output = Vec::new();
    for (section_ordinal, link) in crate::native::features::canonical_feature_history_links(
        ctx,
        segment_om_links(ctx, container)?,
    )?
    .into_iter()
    .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX state status section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
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
            let body = row.into_owned(ctx)?;
            let Some(record) = OmOperationStateStatus::new(
                retained_om_padded_state_id(
                    ctx,
                    "nx:feature-history:operation-state-status#",
                    section_ordinal,
                    ordinal,
                    "NX operation state status id",
                )?,
                ctx.copy_retained_text(&link.id, "NX state status section link")?,
                ordinal,
                body,
                ctx.copy_retained_text(&entry.name, "NX state status source entry")?,
                source_offset,
            ) else {
                continue;
            };
            ctx.charge_collection_items(1, "NX operation state statuses")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OmOperationStateStatus>()),
                "retain NX operation state status",
            )?;
            output.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX operation state statuses", 0, 1)
            })?;
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
    let sections = container.om_sections(ctx)?;
    let mut output = Vec::new();
    for (section_ordinal, link) in crate::native::features::canonical_feature_history_links(
        ctx,
        segment_om_links(ctx, container)?,
    )?
    .into_iter()
    .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX state slot section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
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
            ctx.charge_collection_items(1, "NX operation state slot lanes")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<OmOperationStateSlotLane>(),
                ),
                "retain NX operation state slot lane",
            )?;
            output.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX operation state slot lanes", 0, 1)
            })?;
            output.push(OmOperationStateSlotLane {
                id: retained_om_padded_state_id(
                    ctx,
                    "nx:feature-history:operation-state-slot-lane#",
                    section_ordinal,
                    ordinal,
                    "NX operation state slot lane id",
                )?,
                section_link: ctx.copy_retained_text(&link.id, "NX state slot section link")?,
                ordinal,
                frame,
                source_entry: ctx.copy_retained_text(&entry.name, "NX state slot source entry")?,
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
    pub(super) fn property_name(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        let name = match self {
            Self::Millimeter => "millimeter",
            Self::Inch => "inch",
            Self::Degree => "degree",
            Self::Native(unit) => unit.as_str(),
        };
        ctx.copy_retained_text(name, "NX expression unit property")
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
pub(super) struct Expression {
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
    #[allow(clippy::struct_field_names)]
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

impl Serialize for Expression {
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
            value: value.value.map(FiniteReal::get),
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
    ctx: &DecodeContext<'_>,
    expression: &str,
    mut parameter_value: impl FnMut(&str) -> Option<f64>,
) -> Result<Option<FiniteReal>, CodecError> {
    struct NumberText {
        bytes: [u8; 400],
        len: usize,
    }

    impl std::fmt::Write for NumberText {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            let end = self.len.checked_add(text.len()).ok_or(std::fmt::Error)?;
            let slot = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
            slot.copy_from_slice(text.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    let bytes = expression.as_bytes();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "NX expression substitution",
    )?;
    let mut reservation = ctx.reserve_scoped(0, "NX expression substitution")?;
    let mut substituted = String::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if let Some(end) = expression_parameter_reference_end(bytes, at) {
            let Some(value) = parameter_value(&expression[at..end]) else {
                return Ok(None);
            };
            let mut number = NumberText {
                bytes: [0; 400],
                len: 0,
            };
            std::fmt::Write::write_fmt(&mut number, format_args!("{value}"))
                .map_err(|_| ctx.refuse_codec_limit("NX expression number formatting", 0, 400))?;
            let value_text = std::str::from_utf8(&number.bytes[..number.len]).map_err(|_| {
                ctx.refuse_codec_limit(
                    "NX expression number formatting",
                    0,
                    cadmpeg_core::decode::u64_from_index(number.len),
                )
            })?;
            let added = value_text
                .len()
                .checked_add(2)
                .ok_or_else(|| ctx.refuse_codec_limit("NX expression substitution", 0, u64::MAX))?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(added))?;
            substituted.try_reserve(added).map_err(|_| {
                ctx.refuse_codec_limit(
                    "NX expression substitution",
                    0,
                    cadmpeg_core::decode::u64_from_index(added),
                )
            })?;
            substituted.push('(');
            substituted.push_str(value_text);
            substituted.push(')');
            at = end;
        } else {
            reservation.grow(1)?;
            substituted
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("NX expression substitution", 0, 1))?;
            substituted.push(char::from(bytes[at]));
            at += 1;
        }
    }
    crate::om::evaluate_constant_expression(ctx, &substituted)
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
    registry_suffix: Vec<u8>,
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
    registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layout_prefix: Vec<u8>,
    /// Stable eight-byte class fingerprint in a framed registry suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_schema_fingerprint"
    )]
    schema_fingerprint: Option<[u8; 8]>,
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
    registry_suffix: Vec<u8>,
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
    registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layout_prefix: Vec<u8>,
    /// Stable eight-byte field fingerprint in a framed registry suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_schema_fingerprint"
    )]
    schema_fingerprint: Option<[u8; 8]>,
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
    sha256: crate::native::hex::Sha256Hex,
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
    sha256: &'a crate::native::hex::Sha256Hex,
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
    sha256: crate::native::hex::Sha256Hex,
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
fn stable_object_record_identity(
    ctx: &DecodeContext<'_>,
    source_entry: &str,
    bytes: &[u8],
) -> Result<String, CodecError> {
    let work = source_entry
        .len()
        .checked_add(bytes.len())
        .and_then(|length| length.checked_add(b"nx:om:object-record\0".len() + 1))
        .ok_or_else(|| ctx.refuse_codec_limit("NX object record identity digest", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "NX object record identity digest",
    )?;
    let mut digest = Sha256::new();
    digest.update(b"nx:om:object-record\0");
    digest.update(source_entry.as_bytes());
    digest.update([0]);
    digest.update(bytes);
    let digest: [u8; 32] = digest.finalize().into();
    data_block_hex(
        ctx,
        &digest,
        "nx:om:object-record:",
        "NX object record identity",
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
fn stable_object_record_identities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source_entry: &str,
    records: &[&[u8]],
) -> Result<Vec<Option<String>>, cadmpeg_core::CodecError> {
    const MAX_GRAPH_WORK: usize = 8 * 1024 * 1024;

    let mut reference_reservation = ctx.reserve_scoped(0, "NX object record graph references")?;
    let mut references = Vec::new();
    for bytes in records {
        ctx.charge_collection_items(1, "NX object record graph reference lists")?;
        reference_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Vec<(usize, usize)>,
        >()))?;
        references.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX object record graph reference lists", 0, 1)
        })?;
        let parsed = crate::om::counted_record_references(ctx, bytes, 0, records.len())?;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(parsed.len()),
            "NX object record graph references",
        )?;
        let pair_bytes = parsed
            .len()
            .checked_mul(std::mem::size_of::<(usize, usize)>())
            .ok_or_else(|| ctx.refuse_codec_limit("NX object record graph references", 0, 1))?;
        reference_reservation.grow(cadmpeg_core::decode::u64_from_index(pair_bytes))?;
        let mut pairs = Vec::new();
        pairs.try_reserve_exact(parsed.len()).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX object record graph references", 0, 1)
        })?;
        for reference in parsed {
            pairs.push((reference.offset, usize::from(reference.value)));
        }
        references.push(pairs);
    }
    let mut graph_work = MAX_GRAPH_WORK;
    let output_bytes = records
        .len()
        .checked_mul(std::mem::size_of::<Option<String>>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX object record identities", 0, 1))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(records.len()),
        "NX object record identities",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(output_bytes),
        "NX object record identities",
    )?;
    let mut identities = Vec::new();
    identities
        .try_reserve_exact(records.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX object record identities", 0, 1))?;
    for root in 0..records.len() {
        let identity = if references[root].is_empty() {
            Some(stable_object_record_identity(
                ctx,
                source_entry,
                records[root],
            )?)
        } else {
            stable_object_record_graph_identity(
                ctx,
                source_entry,
                records,
                &references,
                root,
                &mut graph_work,
            )?
        };
        identities.push(identity);
    }
    Ok(identities)
}

fn consume_stable_object_graph_work(
    ctx: &DecodeContext<'_>,
    work: &mut usize,
    amount: usize,
) -> Result<Option<()>, CodecError> {
    let Some(remaining) = work.checked_sub(amount) else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(amount),
        "NX object record graph identity",
    )?;
    *work = remaining;
    Ok(Some(()))
}

fn append_stable_object_graph_node(
    ctx: &DecodeContext<'_>,
    digest: &mut Sha256,
    graph_work: &mut usize,
    node_id: u64,
) -> Result<Option<()>, CodecError> {
    let Some(()) = consume_stable_object_graph_work(ctx, graph_work, 9)? else {
        return Ok(None);
    };
    digest.update([STABLE_GRAPH_NODE_START]);
    digest.update(node_id.to_le_bytes());
    Ok(Some(()))
}

const STABLE_GRAPH_NODE_START: u8 = 0xf0;

/// Encode one rooted object-record graph without depending on local ordinals.
fn stable_object_record_graph_identity(
    ctx: &DecodeContext<'_>,
    source_entry: &str,
    records: &[&[u8]],
    references: &[Vec<(usize, usize)>],
    root: usize,
    graph_work: &mut usize,
) -> Result<Option<String>, CodecError> {
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

    let Some(seed_work) = source_entry.len().checked_add(32) else {
        return Ok(None);
    };
    let Some(()) = consume_stable_object_graph_work(ctx, graph_work, seed_work)? else {
        return Ok(None);
    };
    let mut digest = Sha256::new();
    digest.update(b"nx:om:object-record-graph\0");
    digest.update(cadmpeg_core::decode::u64_from_index(source_entry.len()).to_le_bytes());
    digest.update(source_entry.as_bytes());

    let mut node_ids = BTreeMap::<usize, u64>::new();
    let mut next_node_id = 0_u64;
    let mut stack = Vec::new();
    let mut stack_charged_len = 0usize;
    let mut map_reservation = ctx.reserve_scoped(0, "NX object record graph nodes")?;
    let mut stack_reservation = ctx.reserve_scoped(0, "NX object record graph stack")?;

    ctx.charge_collection_items(1, "NX object record graph nodes")?;
    map_reservation.grow(cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<(usize, u64)>() * 4,
    ))?;
    node_ids.insert(root, next_node_id);
    let Some(()) = append_stable_object_graph_node(ctx, &mut digest, graph_work, next_node_id)?
    else {
        return Ok(None);
    };
    let Some(next) = next_node_id.checked_add(1) else {
        return Ok(None);
    };
    next_node_id = next;
    ctx.charge_collection_items(1, "NX object record graph stack")?;
    stack_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
        Frame,
    >()))?;
    stack_charged_len += 1;
    stack
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX object record graph stack", 0, 1))?;
    stack.push(Frame {
        record: root,
        next_reference: 0,
        raw_cursor: 0,
    });

    while let Some(frame_index) = stack.len().checked_sub(1) {
        let (record, next_reference, raw_cursor) = {
            let Some(frame) = stack.get(frame_index) else {
                return Ok(None);
            };
            (frame.record, frame.next_reference, frame.raw_cursor)
        };
        let Some(&record_bytes) = records.get(record) else {
            return Ok(None);
        };
        let Some(record_references) = references.get(record) else {
            return Ok(None);
        };
        if let Some(&(reference_offset, target)) = record_references.get(next_reference) {
            let Some(reference_end) = reference_offset.checked_add(3) else {
                return Ok(None);
            };
            if reference_offset < raw_cursor
                || reference_end > record_bytes.len()
                || target >= records.len()
            {
                return Ok(None);
            }
            let Some(raw) = record_bytes.get(raw_cursor..reference_offset) else {
                return Ok(None);
            };
            let Some(raw_work) = raw.len().checked_add(9) else {
                return Ok(None);
            };
            let Some(()) = consume_stable_object_graph_work(ctx, graph_work, raw_work)? else {
                return Ok(None);
            };
            digest.update([RAW]);
            digest.update(cadmpeg_core::decode::u64_from_index(raw.len()).to_le_bytes());
            digest.update(raw);

            let Some(frame) = stack.get_mut(frame_index) else {
                return Ok(None);
            };
            let Some(next_reference) = frame.next_reference.checked_add(1) else {
                return Ok(None);
            };
            frame.next_reference = next_reference;
            frame.raw_cursor = reference_end;

            if let Some(&target_id) = node_ids.get(&target) {
                let Some(()) = consume_stable_object_graph_work(ctx, graph_work, 9)? else {
                    return Ok(None);
                };
                digest.update([REFERENCE_BACK]);
                digest.update(target_id.to_le_bytes());
            } else {
                ctx.charge_collection_items(1, "NX object record graph nodes")?;
                map_reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<(usize, u64)>() * 4,
                ))?;
                node_ids.insert(target, next_node_id);
                digest.update([REFERENCE_NEW]);
                digest.update(next_node_id.to_le_bytes());
                let Some(()) =
                    append_stable_object_graph_node(ctx, &mut digest, graph_work, next_node_id)?
                else {
                    return Ok(None);
                };
                let Some(next) = next_node_id.checked_add(1) else {
                    return Ok(None);
                };
                next_node_id = next;
                if stack.len() >= stack_charged_len {
                    ctx.charge_collection_items(1, "NX object record graph stack")?;
                    stack_reservation.grow(cadmpeg_core::decode::u64_from_index(
                        std::mem::size_of::<Frame>(),
                    ))?;
                    stack_charged_len += 1;
                }
                stack.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("allocate NX object record graph stack", 0, 1)
                })?;
                stack.push(Frame {
                    record: target,
                    next_reference: 0,
                    raw_cursor: 0,
                });
            }
        } else {
            if raw_cursor > record_bytes.len() {
                return Ok(None);
            }
            let Some(raw) = record_bytes.get(raw_cursor..) else {
                return Ok(None);
            };
            let Some(raw_work) = raw.len().checked_add(1) else {
                return Ok(None);
            };
            let Some(()) = consume_stable_object_graph_work(ctx, graph_work, raw_work)? else {
                return Ok(None);
            };
            digest.update([RAW]);
            digest.update(cadmpeg_core::decode::u64_from_index(raw.len()).to_le_bytes());
            digest.update(raw);
            digest.update([NODE_END]);
            stack.pop();
        }
    }

    let digest: [u8; 32] = digest.finalize().into();
    data_block_hex(
        ctx,
        &digest,
        "nx:om:object-record:",
        "NX object record graph identity",
    )
    .map(Some)
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
    pub(super) sha256: crate::native::hex::Sha256Hex,
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
    raw_object_id: &'a [u8],
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
            raw_object_id: self.object.raw(),
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
    raw_object_id: Vec<u8>,
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
fn data_block_digest(
    ctx: &DecodeContext<'_>,
    source_entry: &str,
    role: DataBlockRole,
    bytes: &[u8],
) -> Result<[u8; 32], CodecError> {
    let work = source_entry
        .len()
        .checked_add(bytes.len())
        .and_then(|sum| sum.checked_add(18))
        .ok_or_else(|| ctx.refuse_codec_limit("nx data block identity digest", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "nx data block identity digest",
    )?;
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
    Ok(digest.finalize().into())
}

fn data_block_hex(
    ctx: &DecodeContext<'_>,
    digest: &[u8; 32],
    prefix: &'static str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let length = prefix
        .len()
        .checked_add(64)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    text.push_str(prefix);
    for byte in digest {
        write!(&mut text, "{byte:02x}").map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    Ok(text)
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
    sha256: crate::native::hex::Sha256Hex,
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
    let document_bytes = payload
        .len()
        .checked_mul(16)
        .ok_or_else(|| ctx.refuse_codec_limit("NX material catalog XML size", 0, 1))?;
    let _document_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(document_bytes),
        "NX material catalog XML",
    )?;
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return Ok(None);
    };
    let root = document.root_element();
    if root.tag_name().name() != "folderContents" {
        return Ok(None);
    }
    let index_bytes = assets
        .len()
        .checked_mul(
            std::mem::size_of::<(&str, &MaterialTextureAsset)>() * 4
                + std::mem::size_of::<&str>() * 4,
        )
        .ok_or_else(|| ctx.refuse_codec_limit("NX material catalog index size", 0, 1))?;
    let _index_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "NX material catalog index",
    )?;
    let mut assets_by_path = BTreeMap::new();
    for asset in assets {
        if !assets_by_path.contains_key(asset.storage_path()) {
            ctx.charge_collection_items(1, "NX material asset path index")?;
        }
        assets_by_path.insert(asset.storage_path(), asset);
    }
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
        if seen_assets.contains(asset.id.as_str()) {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "NX material catalog seen assets")?;
        seen_assets.insert(asset.id.as_str());
        let ordinal = catalog.len();
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(node.range().start))
            .ok_or_else(|| ctx.refuse_codec_limit("NX material catalog source offset", 0, 1))?;
        ctx.charge_collection_items(1, "NX material catalog entries")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<MaterialTextureCatalogEntry>()), "retain NX material catalog entry")?;
        catalog
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX material catalog entries", 0, 1))?;
        catalog.push(MaterialTextureCatalogEntry {
            id: retained_om_index_id(
                ctx,
                "nx:qafmetadata-",
                entry_index,
                ":material-texture#",
                cadmpeg_core::decode::u64_from_index(ordinal),
                "NX material catalog entry id",
            )?,
            texture_asset: ctx.copy_retained_text(&asset.id, "NX material catalog texture asset")?,
            storage_path: ctx.copy_retained_text(storage_path, "NX material catalog storage path")?,
            material_path: ctx.copy_retained_text(material_path, "NX material catalog material path")?,
            create_time: ctx.copy_retained_text(create_time, "NX material catalog create time")?,
            modify_time: ctx.copy_retained_text(modify_time, "NX material catalog modify time")?,
            source_entry: ctx.copy_retained_text(source_entry, "NX material catalog source entry")?,
            source_offset,
        });
    }
    Ok(Some(catalog))
}

/// Decode end-anchored external child-part string tables.
pub(super) fn external_references(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ExternalReference>, cadmpeg_core::CodecError> {
    let strings = container.external_reference_strings(ctx)?;
    let count = strings.len();
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    let ordinal_bytes = strings
        .iter()
        .try_fold(0usize, |total, (entry, _, _)| {
            total.checked_add(entry.name.len())
        })
        .and_then(|text_bytes| {
            count
                .checked_mul(std::mem::size_of::<(String, u32)>())
                .and_then(|slots| slots.checked_add(text_bytes))
        })
        .ok_or_else(|| ctx.refuse_codec_limit("nx external reference ordinals", 0, count_u64))?;
    let _ordinal_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(ordinal_bytes),
        "nx external reference ordinals",
    )?;
    ctx.charge_collection_items(count_u64, "nx external references")?;
    let record_bytes = count
        .checked_mul(std::mem::size_of::<ExternalReference>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx external references", 0, count_u64))?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(record_bytes),
        "nx external references",
    )?;
    let mut ordinals = BTreeMap::<String, u32>::new();
    let mut references = Vec::new();
    references
        .try_reserve_exact(count)
        .map_err(|_| ctx.refuse_codec_limit("nx external references", 0, count_u64))?;
    for (entry, relative, path) in strings {
        let current = if let Some(ordinal) = ordinals.get_mut(entry.name.as_str()) {
            let current = *ordinal;
            *ordinal = ordinal.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("nx external reference ordinal", 0, count_u64)
            })?;
            current
        } else {
            ctx.charge_collection_items(1, "nx external reference ordinals")?;
            let mut key = String::new();
            key.try_reserve_exact(entry.name.len()).map_err(|_| {
                ctx.refuse_codec_limit("nx external reference ordinals", 0, count_u64)
            })?;
            key.push_str(&entry.name);
            ordinals.insert(key, 1);
            0
        };
        let digits = if current == 0 {
            1
        } else {
            usize::try_from(current.ilog10()).map_err(|_| {
                ctx.refuse_codec_limit("nx external reference identity", 0, count_u64)
            })? + 1
        };
        let id_len = "nx:external-reference:"
            .len()
            .checked_add(entry.name.len())
            .and_then(|len| len.checked_add(1))
            .and_then(|len| len.checked_add(digits))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("nx external reference identity", 0, count_u64)
            })?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_len),
            "nx external reference identity",
        )?;
        let mut id = String::new();
        id.try_reserve_exact(id_len)
            .map_err(|_| ctx.refuse_codec_limit("nx external reference identity", 0, count_u64))?;
        write!(&mut id, "nx:external-reference:{}#{current}", entry.name)
            .map_err(|_| ctx.refuse_codec_limit("nx external reference identity", 0, count_u64))?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(entry.name.len()),
            "nx external reference source entry",
        )?;
        let mut source_entry = String::new();
        source_entry
            .try_reserve_exact(entry.name.len())
            .map_err(|_| {
                ctx.refuse_codec_limit("nx external reference source entry", 0, count_u64)
            })?;
        source_entry.push_str(&entry.name);
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        references.push(ExternalReference {
            id,
            ordinal: current,
            path,
            source_entry,
            source_offset: entry_offset + relative as u64,
        });
    }
    Ok(references)
}

/// Decode exact indexed external-reference record prefixes.
pub(super) fn external_reference_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ExternalReferenceRecord>, cadmpeg_core::CodecError> {
    let parsed = container.external_reference_records(ctx)?;
    let count = parsed.len();
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    ctx.charge_collection_items(count_u64, "nx native external reference records")?;
    let bytes = count
        .checked_mul(std::mem::size_of::<ExternalReferenceRecord>())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("nx native external reference records", 0, count_u64)
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(bytes),
        "nx native external reference records",
    )?;
    let mut output = Vec::new();
    output.try_reserve_exact(count).map_err(|_| {
        ctx.refuse_codec_limit("nx native external reference records", 0, count_u64)
    })?;
    for (entry, record) in parsed {
        let id = external_reference_record_id(
            ctx,
            "nx:external-reference-record:",
            &entry.name,
            record.record_id,
            "nx native external reference record id",
        )?;
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("nx native external reference record offset", 0, 1)
            })?;
        output.push(ExternalReferenceRecord {
            id,
            record_id: record.record_id,
            declared_count: record.declared_count,
            id_slots: record.id_slots,
            handles: record.handles,
            tail_byte_len: cadmpeg_core::decode::u64_from_index(record.tail_byte_len),
            source_entry: ctx.copy_retained_text(&entry.name, "nx native external reference source entry")?,
            source_offset,
        });
    }
    Ok(output)
}

fn external_reference_record_id(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    entry_name: &str,
    record_id: u32,
    operation: &'static str,
) -> Result<String, CodecError> {
    let digits = record_id.checked_ilog10().map_or(1, |n| n + 1);
    let id_len = prefix
        .len()
        .checked_add(entry_name.len())
        .and_then(|len| len.checked_add(1))
        .and_then(|len| len.checked_add(usize::try_from(digits).ok()?))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(id_len), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(id_len)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(&mut id, "{prefix}{entry_name}#{record_id}")
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

/// Retain all indexed records and link uniquely decoded handle-set records.
pub(super) fn external_reference_indexed_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
    decoded: &[ExternalReferenceRecord],
) -> Result<Vec<ExternalReferenceIndexedRecord>, cadmpeg_core::CodecError> {
    let mut decoded_by_key = BTreeMap::<(&str, u32), Option<&ExternalReferenceRecord>>::new();
    let index_bytes = decoded
        .len()
        .checked_mul(std::mem::size_of::<(
            (&str, u32),
            Option<&ExternalReferenceRecord>,
        )>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx external reference decoded index", 0, 1))?;
    let _index_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "nx external reference decoded index",
    )?;
    for record in decoded {
        let key = (record.source_entry.as_str(), record.record_id);
        if let Some(value) = decoded_by_key.get_mut(&key) {
            *value = None;
        } else {
            ctx.charge_collection_items(1, "nx external reference decoded index")?;
            decoded_by_key.insert(key, Some(record));
        }
    }
    let parsed = container.external_reference_indexed_records(ctx)?;
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
        ctx.charge_collection_items(1, "nx native external reference indexed records")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<ExternalReferenceIndexedRecord>(),
            ),
            "nx native external reference indexed records",
        )?;
        output.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("nx native external reference indexed records", 0, 1)
        })?;
        let id = external_reference_record_id(
            ctx,
            "nx:external-reference-indexed-record:",
            &entry.name,
            record.record_id,
            "nx native external reference indexed record id",
        )?;
        let handle_set_record = decoded_by_key
            .get(&(entry.name.as_str(), record.record_id))
            .and_then(|record| *record)
            .map(|record| {
                ctx.copy_retained_text(&record.id, "nx external reference handle-set link")
            })
            .transpose()?;
        output.push(ExternalReferenceIndexedRecord {
            id,
            record_id: record.record_id,
            byte_len,
            sha256: crate::native::hex::Sha256Hex::digest_charged(
                ctx,
                bytes,
                "nx external reference indexed digest",
            )?,
            handle_set_record,
            source_entry: ctx.copy_retained_text(&entry.name, "nx native external reference indexed source entry")?,
            source_offset,
        });
    }
    Ok(output)
}

/// Decode every exact six- or seven-byte empty indexed record.
pub(super) fn external_reference_empty_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
    indexed: &[ExternalReferenceIndexedRecord],
) -> Result<Vec<ExternalReferenceEmptyRecord>, CodecError> {
    let mut output = Vec::new();
    for record in indexed {
        let Some(bytes) = container.bounded_entry_bytes(record.source_offset, record.byte_len)
        else {
            continue;
        };
        let Some(closing_marker) = crate::container::parse_extref_empty_record(bytes) else {
            continue;
        };
        ctx.charge_collection_items(1, "nx native external reference empty records")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<ExternalReferenceEmptyRecord>(),
            ),
            "nx native external reference empty records",
        )?;
        output.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("nx native external reference empty records", 0, 1)
        })?;
        let replacement = record.id.split_once("indexed-record");
        let id_len = if let Some((before, after)) = replacement {
            before
                .len()
                .checked_add("empty-record".len())
                .and_then(|len| len.checked_add(after.len()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("nx native external reference empty record id", 0, 1)
                })?
        } else {
            record.id.len()
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_len),
            "nx native external reference empty record id",
        )?;
        let mut id = String::new();
        id.try_reserve_exact(id_len).map_err(|_| {
            ctx.refuse_codec_limit("nx native external reference empty record id", 0, 1)
        })?;
        if let Some((before, after)) = replacement {
            id.push_str(before);
            id.push_str("empty-record");
            id.push_str(after);
        } else {
            id.push_str(&record.id);
        }
        output.push(ExternalReferenceEmptyRecord {
            id,
            indexed_record: ctx.copy_retained_text(&record.id, "nx native external reference empty record link")?,
            closing_marker,
        });
    }
    Ok(output)
}

/// Decode exact adjacent reference pairs from bounded handle-set tails.
pub(super) fn external_reference_tail_reference_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    records: &[ExternalReferenceRecord],
) -> Result<Vec<ExternalReferenceTailReferencePair>, cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    for record in records {
        let Some(source_offset) = record
            .source_offset
            .checked_add(record.handles.prefix_byte_len() as u64)
        else {
            continue;
        };
        let Some(bytes) = container.bounded_entry_bytes(source_offset, record.tail_byte_len) else {
            continue;
        };
        for (ordinal, (offset, persistent_handle, tagged_reference)) in
            crate::container::parse_extref_reference_pairs(ctx, bytes)?
                .into_iter()
                .enumerate()
        {
            ctx.charge_collection_items(1, "nx native external reference tail pairs")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    ExternalReferenceTailReferencePair,
                >()),
                "nx native external reference tail pairs",
            )?;
            out.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference tail pairs", 0, 1)
            })?;
            let record_key = record
                .id
                .split_once('#')
                .map_or(record.id.as_str(), |(_, key)| key);
            let prefix = "nx:external-reference:tail-reference-pair#";
            let digits = ordinal.checked_ilog10().map_or(1, |n| n + 1);
            let id_len = prefix
                .len()
                .checked_add(record_key.len())
                .and_then(|len| len.checked_add(1))
                .and_then(|len| len.checked_add(usize::try_from(digits).ok()?))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("nx native external reference tail pair id", 0, 1)
                })?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(id_len),
                "nx native external reference tail pair id",
            )?;
            let mut id = String::new();
            id.try_reserve_exact(id_len).map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference tail pair id", 0, 1)
            })?;
            write!(&mut id, "{prefix}{record_key}-{ordinal}").map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference tail pair id", 0, 1)
            })?;
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference tail pair ordinal", 0, 1)
            })?;
            let source_offset = source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("nx native external reference tail pair offset", 0, 1)
                })?;
            out.push(ExternalReferenceTailReferencePair {
                id,
                handle_set_record: ctx.copy_retained_text(&record.id, "nx native external reference tail pair link")?,
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
    ctx: &DecodeContext<'_>,
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
) -> Result<Vec<ExternalReferenceRecordStringUse>, CodecError> {
    let mut references_by_key = BTreeMap::<(&str, u32), Option<&ExternalReference>>::new();
    let index_bytes = references
        .len()
        .checked_mul(std::mem::size_of::<((&str, u32), Option<&ExternalReference>)>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx external reference slot index", 0, 1))?;
    let _index_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "nx external reference slot index",
    )?;
    for reference in references {
        ctx.charge_work(1, "nx external reference slot index")?;
        let key = (reference.source_entry.as_str(), reference.ordinal);
        if let Some(value) = references_by_key.get_mut(&key) {
            *value = None;
        } else {
            ctx.charge_collection_items(1, "nx external reference slot index")?;
            references_by_key.insert(key, Some(reference));
        }
    }
    let mut output = Vec::new();
    for record in records {
        ctx.charge_work(1, "nx external reference slot resolution")?;
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
            ctx.charge_collection_items(1, "nx native external reference string uses")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    ExternalReferenceRecordStringUse,
                >()),
                "nx native external reference string uses",
            )?;
            output.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference string uses", 0, 1)
            })?;
            let record_key = record
                .id
                .split_once('#')
                .map_or(record.id.as_str(), |(_, key)| key);
            let prefix = "nx:external-reference:record-string-use#";
            let id_len = prefix
                .len()
                .checked_add(record_key.len())
                .and_then(|len| len.checked_add(2))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("nx native external reference string use id", 0, 1)
                })?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(id_len),
                "nx native external reference string use id",
            )?;
            let mut id = String::new();
            id.try_reserve_exact(id_len).map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference string use id", 0, 1)
            })?;
            write!(&mut id, "{prefix}{record_key}-{}", u8::from(slot)).map_err(|_| {
                ctx.refuse_codec_limit("nx native external reference string use id", 0, 1)
            })?;
            let source_offset =
                record
                    .source_offset
                    .checked_add(slot.offset())
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "nx native external reference string use offset",
                            0,
                            1,
                        )
                    })?;
            output.push(ExternalReferenceRecordStringUse {
                id,
                external_record: ctx.copy_retained_text(&record.id, "nx native external reference string use record")?,
                slot,
                string_index: record.id_slots[slot.index()],
                external_reference: ctx.copy_retained_text(&reference.id, "nx native external reference string use target")?,
                source_offset,
            });
        }
    }
    Ok(output)
}

/// Bind complete record lanes to their slot-zero name and slot-two directory.
pub(super) fn external_reference_record_children(
    ctx: &DecodeContext<'_>,
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
    uses: &[ExternalReferenceRecordStringUse],
) -> Result<Vec<ExternalReferenceRecordChild>, CodecError> {
    let mut references_by_id = BTreeMap::<&str, Option<&ExternalReference>>::new();
    let index_bytes = references
        .len()
        .checked_mul(std::mem::size_of::<(&str, Option<&ExternalReference>)>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx external reference child index", 0, 1))?;
    let _index_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "nx external reference child index",
    )?;
    for reference in references {
        ctx.charge_work(1, "nx external reference child index")?;
        if let Some(value) = references_by_id.get_mut(reference.id.as_str()) {
            *value = None;
        } else {
            ctx.charge_collection_items(1, "nx external reference child index")?;
            references_by_id.insert(reference.id.as_str(), Some(reference));
        }
    }
    let mut output = Vec::new();
    for record in records {
        let mut record_uses: [Option<&ExternalReferenceRecordStringUse>; 4] = [None; 4];
        let mut count = 0usize;
        let mut duplicate = false;
        for use_ in uses {
            ctx.charge_work(1, "nx external reference child use scan")?;
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
        ctx.charge_collection_items(1, "nx native external reference children")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<ExternalReferenceRecordChild>(),
            ),
            "nx native external reference children",
        )?;
        output
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx native external reference children", 0, 1))?;
        let id_len =
            record.id.len().checked_add(":child".len()).ok_or_else(|| {
                ctx.refuse_codec_limit("nx native external reference child id", 0, 1)
            })?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_len),
            "nx native external reference child id",
        )?;
        let mut id = String::new();
        id.try_reserve_exact(id_len)
            .map_err(|_| ctx.refuse_codec_limit("nx native external reference child id", 0, 1))?;
        write!(&mut id, "{}:child", record.id)
            .map_err(|_| ctx.refuse_codec_limit("nx native external reference child id", 0, 1))?;
        output.push(ExternalReferenceRecordChild {
            id,
            external_record: ctx.copy_retained_text(&record.id, "nx external reference child record")?,
            name_reference: ctx.copy_retained_text(&name.id, "nx external reference child name")?,
            directory_reference: ctx.copy_retained_text(&directory.id, "nx external reference child directory")?,
        });
    }
    Ok(output)
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(xml.len()),
        "nx arrangement XML scan",
    )?;
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return Ok(Vec::new());
    };
    let root = document.root_element();
    if root.tag_name().name() != "Arrangements" {
        return Ok(Vec::new());
    }
    let node_count = root.children().filter(roxmltree::Node::is_element).count();
    let index_bytes = node_count
        .checked_mul(std::mem::size_of::<&str>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx arrangement names", 0, 1))?;
    let _names_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "nx arrangement names",
    )?;
    let mut names = BTreeSet::new();
    let mut active_count = 0usize;
    for node in root.children().filter(roxmltree::Node::is_element) {
        ctx.charge_work(1, "nx arrangement records")?;
        if node.tag_name().name() != "Arrangement" {
            return Ok(Vec::new());
        }
        let Some(name) = node.attribute("Name") else {
            return Ok(Vec::new());
        };
        if name.is_empty() || names.contains(name) {
            return Ok(Vec::new());
        }
        ctx.charge_collection_items(1, "nx arrangement names")?;
        names.insert(name);
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
    let count_u64 = cadmpeg_core::decode::u64_from_index(node_count);
    ctx.charge_collection_items(count_u64, "nx arrangement configurations")?;
    let record_bytes = node_count
        .checked_mul(std::mem::size_of::<Configuration>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx arrangement configurations", 0, count_u64))?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(record_bytes),
        "nx arrangement configurations",
    )?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(node_count)
        .map_err(|_| ctx.refuse_codec_limit("nx arrangement configurations", 0, count_u64))?;
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
            .ok_or_else(|| ctx.refuse_codec_limit("nx arrangement source offset", 0, 1))?;
        output.push(Configuration {
            id: retained_om_index_id(
                ctx,
                "nx:arrangements-",
                entry_index,
                ":configuration#",
                cadmpeg_core::decode::u64_from_index(ordinal),
                "nx arrangement configuration id",
            )?,
            name: ctx.copy_retained_text(name, "nx arrangement configuration name")?,
            is_default,
            source_entry: ctx.copy_retained_text(&entry.name, "nx arrangement source entry")?,
            source_offset,
        });
    }
    Ok(output)
}

/// Join the two independently framed active-arrangement declarations.
pub(super) fn configuration_attribute_uses(
    ctx: &DecodeContext<'_>,
    configurations: &[Configuration],
    attributes: &[PartAttribute],
) -> Result<Vec<ConfigurationAttributeUse>, CodecError> {
    let mut active = None;
    for configuration in configurations {
        ctx.charge_work(1, "nx active configuration join")?;
        if configuration.is_default && active.replace(configuration).is_some() {
            return Ok(Vec::new());
        }
    }
    let mut declaration = None;
    for attribute in attributes {
        ctx.charge_work(1, "nx active configuration join")?;
        if attribute.owner == "part"
            && attribute.title == "NX_Arrangement"
            && attribute.value_type == "StringAttributeType"
            && declaration.replace(attribute).is_some()
        {
            return Ok(Vec::new());
        }
    }
    let (Some(configuration), Some(attribute)) = (active, declaration) else {
        return Ok(Vec::new());
    };
    if configuration.name != attribute.value {
        return Ok(Vec::new());
    }
    ctx.charge_collection_items(1, "nx active configuration attribute uses")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<ConfigurationAttributeUse>()),
        "nx active configuration attribute uses",
    )?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(1)
        .map_err(|_| ctx.refuse_codec_limit("nx active configuration attribute uses", 0, 1))?;
    output.push(ConfigurationAttributeUse {
        id: ctx.copy_retained_text("nx:arrangements:active-attribute-use#0", "nx active configuration attribute use id")?,
        configuration: ctx.copy_retained_text(&configuration.id, "nx active configuration link")?,
        part_attribute: ctx.copy_retained_text(&attribute.id, "nx active attribute link")?,
        name: ctx.copy_retained_text(&configuration.name, "nx active configuration name")?,
    });
    Ok(output)
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(xml.len()),
        "nx part attribute XML scan",
    )?;
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return Ok(None);
    };
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
    let count = root.children().filter(roxmltree::Node::is_element).count();
    for node in root.children().filter(roxmltree::Node::is_element) {
        ctx.charge_work(1, "nx part attribute records")?;
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
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    ctx.charge_collection_items(count_u64, "nx native part attributes")?;
    let record_bytes = count
        .checked_mul(std::mem::size_of::<PartAttribute>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx native part attributes", 0, count_u64))?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(record_bytes),
        "nx native part attributes",
    )?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| ctx.refuse_codec_limit("nx native part attributes", 0, count_u64))?;
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
            .ok_or_else(|| ctx.refuse_codec_limit("nx part attribute source offset", 0, 1))?;
        output.push(PartAttribute {
            id: retained_om_index_id(
                ctx,
                "nx:part-attributes-",
                entry_index,
                ":attribute#",
                cadmpeg_core::decode::u64_from_index(ordinal),
                "nx native part attribute id",
            )?,
            owner: ctx.copy_retained_text(owner, "nx part attribute owner")?,
            title: ctx.copy_retained_text(title, "nx part attribute title")?,
            value: ctx.copy_retained_text(value, "nx part attribute value")?,
            value_type: ctx.copy_retained_text(value_type, "nx part attribute type")?,
            pdm_based,
            version,
            source_entry: ctx.copy_retained_text(source_entry, "nx part attribute source entry")?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    kind: RegistryKind,
    project: impl Fn(RegistryDefinition) -> T,
) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let framed_sections = container.om_sections(ctx)?;
    let indexed_sections = container.indexed_om_sections(ctx)?;
    let maximum = framed_sections
        .iter()
        .map(|(_, section)| match kind {
            RegistryKind::Class => section.types.len(),
            RegistryKind::Field => section.fields.len(),
        })
        .chain(indexed_sections.iter().map(|(_, section)| match kind {
            RegistryKind::Class => section.types.len(),
            RegistryKind::Field => section.fields.len(),
        }))
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| ctx.refuse_codec_limit("nx registry definition index", 0, 1))?;
    let index_bytes = maximum
        .checked_mul(std::mem::size_of::<((usize, usize), T)>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx registry definition index", 0, 1))?;
    let _index_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "nx registry definition index",
    )?;
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
            ctx.charge_work(1, "nx registry definitions")?;
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
            ctx.charge_collection_items(1, "nx registry definition index")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<T>()),
                "nx registry definition records",
            )?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("nx registry definition ordinal", 0, 1))?;
            let section_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(section_offset))
                .ok_or_else(|| ctx.refuse_codec_limit("nx registry section offset", 0, 1))?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                .ok_or_else(|| ctx.refuse_codec_limit("nx registry definition offset", 0, 1))?;
            definitions.insert(
                key,
                project(RegistryDefinition {
                    id: retained_om_index_id(
                        ctx,
                        "nx:om-entry-",
                        entry_index,
                        marker,
                        cadmpeg_core::decode::u64_from_index(offset),
                        "nx registry definition id",
                    )?,
                    name: ctx.copy_retained_text(name, "nx registry definition name")?,
                    ordinal,
                    trailing_code,
                    registry_suffix: ctx.copy_retained(registry_suffix, "nx registry suffix")?,
                    section_offset,
                    source_entry: ctx.copy_retained_text(&entry.name, "nx registry source entry")?,
                    source_offset,
                }),
            );
        }
    }
    let count = definitions.len();
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    ctx.charge_collection_items(count_u64, "nx registry definition output")?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| ctx.refuse_codec_limit("nx registry definition output", 0, count_u64))?;
    output.extend(definitions.into_values());
    Ok(output)
}

/// Decode class definitions from every framed OM section.
pub(super) fn class_definitions(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ClassDefinition>, CodecError> {
    registry_definitions(ctx, container, RegistryKind::Class, |d| ClassDefinition {
        id: d.id,
        name: d.name,
        ordinal: d.ordinal,
        trailing_code: d.trailing_code,
        registry_suffix: d.registry_suffix,
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
pub(super) fn field_definitions(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FieldDefinition>, CodecError> {
    registry_definitions(ctx, container, RegistryKind::Field, |d| FieldDefinition {
        id: d.id,
        name: d.name,
        ordinal: d.ordinal,
        trailing_code: d.trailing_code,
        registry_suffix: d.registry_suffix,
        section_offset: d.section_offset,
        source_entry: d.source_entry,
        source_offset: d.source_offset,
    })
}

/// Insert one distinct graph relation with caller-budget admission.
fn add_object_record_relation(
    ctx: &DecodeContext<'_>,
    relations: &mut BTreeMap<usize, Vec<usize>>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    source: usize,
    target: usize,
) -> Result<(), CodecError> {
    if !relations.contains_key(&source) {
        ctx.charge_collection_items(1, "NX object record relation index")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(usize, Vec<usize>)>() * 4,
        ))?;
    }
    let related = relations.entry(source).or_default();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(related.len()),
        "NX object record relation deduplication",
    )?;
    if !related.contains(&target) {
        ctx.charge_collection_items(1, "NX object record relations")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            usize,
        >()))?;
        related
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX object record relations", 0, 1))?;
        related.push(target);
    }
    Ok(())
}

fn object_record_relation_ids(
    ctx: &DecodeContext<'_>,
    relations: Option<&Vec<usize>>,
    section_ordinal: usize,
) -> Result<Vec<String>, CodecError> {
    let related = relations.map_or(&[][..], Vec::as_slice);
    let slots = related
        .len()
        .checked_mul(std::mem::size_of::<String>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX object record relation IDs", 0, 1))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(related.len()),
        "NX object record relation IDs",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(slots),
        "NX object record relation IDs",
    )?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(related.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX object record relation IDs", 0, 1))?;
    for &ordinal in related {
        ids.push(retained_om_index_id(
            ctx,
            "nx:om-record-directory-",
            section_ordinal,
            ":entry#",
            cadmpeg_core::decode::u64_from_index(ordinal),
            "NX object record relation ID",
        )?);
    }
    Ok(ids)
}

/// Catalog every externally bounded NX OM entity record.
pub(super) fn object_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ObjectRecord>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(section.base_offset()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX object record section offset", 0, 1))?;
        let record_bytes_len = records
            .len()
            .checked_mul(std::mem::size_of::<&[u8]>())
            .ok_or_else(|| ctx.refuse_codec_limit("NX object record byte views", 0, 1))?;
        let _record_bytes_guard = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(record_bytes_len),
            "NX object record byte views",
        )?;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(records.len()),
            "NX object record byte views",
        )?;
        let mut record_bytes = Vec::new();
        record_bytes
            .try_reserve_exact(records.len())
            .map_err(|_| ctx.refuse_codec_limit("allocate NX object record byte views", 0, 1))?;
        for record in records {
            record_bytes.push(record.bytes);
        }
        let stable_identities = stable_object_record_identities(ctx, &entry.name, &record_bytes)?;
        let mut dependencies = BTreeMap::<usize, Vec<usize>>::new();
        let mut dependents = BTreeMap::<usize, Vec<usize>>::new();
        let mut dependency_guard = ctx.reserve_scoped(0, "NX object record dependency index")?;
        let mut dependent_guard = ctx.reserve_scoped(0, "NX object record dependent index")?;
        for (source, record) in records.iter().enumerate() {
            for reference in record.references(ctx, records.len())? {
                let RecordReference::RecordOrdinal16 { ordinal, .. } = reference.value else {
                    continue;
                };
                let target = usize::from(ordinal);
                add_object_record_relation(
                    ctx,
                    &mut dependencies,
                    &mut dependency_guard,
                    source,
                    target,
                )?;
                add_object_record_relation(
                    ctx,
                    &mut dependents,
                    &mut dependent_guard,
                    target,
                    source,
                )?;
            }
        }
        for (record_ordinal, (record, stable_identity)) in
            records.iter().zip(stable_identities).enumerate()
        {
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
                .ok_or_else(|| ctx.refuse_codec_limit("NX object record source offset", 0, 1))?;
            let object_id_offset = entry_offset
                .checked_add(record.object_id.1)
                .ok_or_else(|| ctx.refuse_codec_limit("NX object record object ID offset", 0, 1))?;
            let section_ordinal_u32 = u32::try_from(section_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX object record section ordinal", 0, 1))?;
            let record_ordinal_u32 = u32::try_from(record_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX object record ordinal", 0, 1))?;
            ctx.charge_collection_items(1, "NX object records")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<ObjectRecord>()),
                "NX object records",
            )?;
            output
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("allocate NX object records", 0, 1))?;
            output.push(ObjectRecord {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-record-directory-",
                    section_ordinal,
                    ":entry#",
                    cadmpeg_core::decode::u64_from_index(record_ordinal),
                    "NX object record ID",
                )?,
                object_id: (record.object_id.0, object_id_offset),
                section_ordinal: section_ordinal_u32,
                record_ordinal: record_ordinal_u32,
                section_offset,
                byte_len: cadmpeg_core::decode::u64_from_index(record.bytes.len()),
                sha256: crate::native::hex::Sha256Hex::digest(record.bytes),
                stable_identity,
                dependencies: object_record_relation_ids(
                    ctx,
                    dependencies.get(&record_ordinal),
                    section_ordinal,
                )?,
                dependents: object_record_relation_ids(
                    ctx,
                    dependents.get(&record_ordinal),
                    section_ordinal,
                )?,
                source_entry: ctx.copy_retained_text(&entry.name, "NX object record source entry")?,
                source_offset,
            });
        }
    }

    let mut identity_counts = BTreeMap::<(&str, &str), usize>::new();
    let mut count_guard = ctx.reserve_scoped(0, "NX object record identity counts")?;
    for record in &output {
        let Some(identity) = record.stable_identity.as_deref() else {
            continue;
        };
        let key = (record.source_entry.as_str(), identity);
        if let Some(count) = identity_counts.get_mut(&key) {
            *count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("NX object record identity count", 0, 1))?;
        } else {
            ctx.charge_collection_items(1, "NX object record identity counts")?;
            count_guard.grow(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<((&str, &str), usize)>() * 4,
            ))?;
            identity_counts.insert(key, 1);
        }
    }
    let flags_bytes = output
        .len()
        .checked_mul(std::mem::size_of::<bool>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX object record identity flags", 0, 1))?;
    let _flags_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(flags_bytes),
        "NX object record identity flags",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(output.len()),
        "NX object record identity flags",
    )?;
    let mut unique_flags = Vec::new();
    unique_flags
        .try_reserve_exact(output.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX object record identity flags", 0, 1))?;
    for record in &output {
        let unique = record.stable_identity.as_deref().is_some_and(|identity| {
            identity_counts.get(&(record.source_entry.as_str(), identity)) == Some(&1)
        });
        unique_flags.push(unique);
    }
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
    let table_id_text = "nx:rmfastload:object-id-table#0";
    let member_id_len = "nx:rmfastload:object-id#".len() + 10;

    ctx.charge_collection_items(count_u64, "admit NX FastLoad identity counts")?;
    let map_entry_bytes = std::mem::size_of::<(u32, usize)>()
        .checked_add(4 * std::mem::size_of::<usize>())
        .ok_or_else(|| {
            CodecError::NotImplemented("FastLoad map entry exceeds address space".into())
        })?;
    let map_bytes = count.checked_mul(map_entry_bytes).ok_or_else(|| {
        CodecError::NotImplemented("FastLoad identity map exceeds address space".into())
    })?;
    let _map_reservation = ctx.reserve_scoped(
        u64::try_from(map_bytes)
            .map_err(|_| CodecError::NotImplemented("FastLoad identity map exceeds u64".into()))?,
        "count NX FastLoad identities",
    )?;
    ctx.charge_work(
        count_u64.checked_mul(2).ok_or_else(|| {
            CodecError::NotImplemented("FastLoad identity work exceeds u64".into())
        })?,
        "count NX FastLoad identities",
    )?;
    let mut counts = HashMap::<u32, usize>::new();
    counts
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX FastLoad identity map", 0, count_u64))?;
    for value in values {
        *counts.entry(*value).or_default() += 1;
    }
    let mut stable_bytes = 0_u64;
    for value in values {
        if counts.get(value) == Some(&1) {
            let digits = if *value == 0 { 1 } else { value.ilog10() + 1 };
            stable_bytes = stable_bytes
                .checked_add(
                    u64::try_from(table_id_text.len() + ":value#".len()).map_err(|_| {
                        CodecError::NotImplemented("FastLoad identity exceeds u64".into())
                    })? + u64::from(digits),
                )
                .ok_or_else(|| {
                    CodecError::NotImplemented("FastLoad identities exceed u64".into())
                })?;
        }
    }

    let native_items = count_u64
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| {
            CodecError::NotImplemented("FastLoad native item count exceeds u64".into())
        })?;
    ctx.charge_collection_items(native_items, "admit NX FastLoad native collections")?;
    ctx.charge_entities(count_u64 + 1, "admit NX FastLoad native entities")?;
    let retained_bytes = count_u64
        .checked_mul(
            u64::try_from(
                std::mem::size_of::<RmFastLoadObjectId>()
                    + std::mem::size_of::<String>()
                    + 2 * member_id_len
                    + table_id_text.len(),
            )
            .map_err(|_| CodecError::NotImplemented("FastLoad native item exceeds u64".into()))?,
        )
        .and_then(|bytes| {
            bytes.checked_add(
                u64::try_from(
                    std::mem::size_of::<RmFastLoadObjectIdTable>()
                        + table_id_text.len()
                        + entry.name.len(),
                )
                .ok()?,
            )
        })
        .and_then(|bytes| bytes.checked_add(stable_bytes))
        .ok_or_else(|| CodecError::NotImplemented("FastLoad native copies exceed u64".into()))?;
    ctx.charge_retained(retained_bytes, "retain NX FastLoad native copies")?;

    let table_id = table_id_text.to_string();
    let mut object_ids = Vec::new();
    object_ids
        .try_reserve_exact(count)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX FastLoad native records", 0, count_u64))?;
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
    let mut member_ids = Vec::new();
    member_ids
        .try_reserve_exact(count)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX FastLoad member links", 0, count_u64))?;
    member_ids.extend(object_ids.iter().map(|object_id| object_id.id.clone()));
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlock>, cadmpeg_core::CodecError> {
    let sections = container.indexed_om_sections(ctx)?;
    let mut count = 0usize;
    for (_, section) in &sections {
        if let Some((_, _, records)) = section.as_offset_only() {
            count = count
                .checked_add(records.len())
                .and_then(|count| count.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit("count NX data blocks", 0, 1))?;
        }
    }
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    let map_bytes = count
        .checked_mul(std::mem::size_of::<([u8; 32], usize)>() * 4)
        .ok_or_else(|| ctx.refuse_codec_limit("reserve NX data block identities", 0, 1))?;
    let _map_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(map_bytes),
        "reserve NX data block identities",
    )?;
    let mut identity_counts = BTreeMap::<[u8; 32], usize>::new();
    for (entry, section) in &sections {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        for (role, block) in std::iter::once((DataBlockRole::Control, control))
            .chain(records.iter().map(|record| (DataBlockRole::Column, record)))
        {
            let digest = data_block_digest(ctx, &entry.name, role, block.bytes)?;
            if !identity_counts.contains_key(&digest) {
                ctx.charge_collection_items(1, "NX data block identities")?;
            }
            let count = identity_counts.entry(digest).or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("count NX data block identities", 0, 1))?;
        }
    }

    ctx.charge_collection_items(count_u64, "NX data block records")?;
    let output_bytes = count
        .checked_mul(std::mem::size_of::<DataBlock>())
        .ok_or_else(|| ctx.refuse_codec_limit("retain NX data block records", 0, 1))?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(output_bytes),
        "retain NX data block records",
    )?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX data block records", 0, count_u64))?;
    for (section_ordinal, (entry, section)) in sections.iter().enumerate() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(section.base_offset()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX data block section offset", 0, 1))?;
        for (block_ordinal, (role, block)) in std::iter::once((DataBlockRole::Control, control))
            .chain(records.iter().map(|record| (DataBlockRole::Column, record)))
            .enumerate()
        {
            let stable_digest = data_block_digest(ctx, &entry.name, role, block.bytes)?;
            let stable_identity = if identity_counts.get(&stable_digest) == Some(&1) {
                Some(data_block_hex(
                    ctx,
                    &stable_digest,
                    "nx:om:data-block:",
                    "retain NX data block identity",
                )?)
            } else {
                None
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(block.bytes.len()),
                "hash NX data block",
            )?;
            let digest = cadmpeg_ir::hash::sha256(block.bytes);
            let sha256 = crate::native::hex::Sha256Hex::try_from(data_block_hex(
                ctx,
                &digest,
                "",
                "retain NX data block digest",
            )?)
            .map_err(|message| CodecError::Malformed(message.into()))?;

            let id_length = "nx:om-data-blocks-"
                .len()
                .checked_add(
                    section_ordinal
                        .checked_ilog10()
                        .map_or(1, |digits| digits as usize + 1),
                )
                .and_then(|length| length.checked_add(":block#".len()))
                .and_then(|length| {
                    length.checked_add(
                        block_ordinal
                            .checked_ilog10()
                            .map_or(1, |digits| digits as usize + 1),
                    )
                })
                .ok_or_else(|| ctx.refuse_codec_limit("retain NX data block id", 0, 1))?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(id_length),
                "retain NX data block id",
            )?;
            let mut id = String::new();
            id.try_reserve_exact(id_length)
                .map_err(|_| ctx.refuse_codec_limit("allocate NX data block id", 0, 1))?;
            write!(
                &mut id,
                "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
            )
            .map_err(|_| ctx.refuse_codec_limit("write NX data block id", 0, 1))?;

            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(entry.name.len()),
                "retain NX data block source entry",
            )?;
            let mut source_entry = String::new();
            source_entry
                .try_reserve_exact(entry.name.len())
                .map_err(|_| ctx.refuse_codec_limit("allocate NX data block source entry", 0, 1))?;
            source_entry.push_str(&entry.name);
            output.push(DataBlock {
                id,
                section_ordinal: u32::try_from(section_ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX data block section ordinal", 0, 1))?,
                block_ordinal: u32::try_from(block_ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX data block ordinal", 0, 1))?,
                role,
                section_offset,
                byte_len: cadmpeg_core::decode::u64_from_index(block.bytes.len()),
                sha256,
                stable_identity,
                source_entry,
                source_offset: entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(block.offset))
                    .ok_or_else(|| ctx.refuse_codec_limit("NX data block source offset", 0, 1))?,
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
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(form) = crate::om::offset_store_control_form(
            ctx,
            control.bytes,
            records.first().map(|record| record.bytes),
        )?
        else {
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
                let Some(byte_len) = std::num::NonZeroU64::new(control.bytes.len() as u64) else {
                    continue;
                };
                DataBlockControlFormKind::ProductAnchored {
                    leading: leading_value,
                    value_count,
                    byte_len,
                }
            }
        };
        let id = retained_om_number_id(
            ctx,
            "nx:om-data-block-control-forms:form#",
            cadmpeg_core::decode::u64_from_index(section_ordinal),
            "retain NX control form id",
        )?;
        let data_block = retained_om_index_id(
            ctx,
            "nx:om-data-blocks-",
            section_ordinal,
            ":block#",
            0,
            "retain NX control form block",
        )?;
        let source_offset = entry
            .file_span()
            .map_or(0, |(offset, _)| offset)
            .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX control form source offset", 0, 1))?;
        ctx.charge_entities(1, "NX data block control form")?;
        ctx.charge_collection_items(1, "NX data block control forms")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<DataBlockControlForm>()),
            "retain NX data block control form",
        )?;
        forms
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX data block control forms", 0, 1))?;
        forms.push(DataBlockControlForm {
            id,
            data_block,
            kind,
            source_offset,
        });
    }
    Ok(forms)
}

struct ScopedOmIndexId<'a> {
    id: String,
    _reservation: cadmpeg_core::decode::ScopedReservation<'a>,
}

fn retained_om_number_id(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    number: u64,
    operation: &'static str,
) -> Result<String, CodecError> {
    use std::fmt::Write;

    let length = prefix
        .len()
        .checked_add(
            number
                .checked_ilog10()
                .map_or(1, |digits| digits as usize + 1),
        )
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(&mut id, "{prefix}{number}").map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

fn scoped_om_index_id<'a>(
    ctx: &'a DecodeContext<'_>,
    prefix: &'static str,
    section_ordinal: usize,
    marker: &'static str,
    ordinal: u32,
    operation: &'static str,
) -> Result<ScopedOmIndexId<'a>, CodecError> {
    use std::fmt::Write;

    let length = prefix
        .len()
        .checked_add(
            section_ordinal
                .checked_ilog10()
                .map_or(1, |digits| digits as usize + 1),
        )
        .and_then(|length| length.checked_add(marker.len()))
        .and_then(|length| {
            length.checked_add(
                ordinal
                    .checked_ilog10()
                    .map_or(1, |digits| digits as usize + 1),
            )
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let reservation =
        ctx.reserve_scoped(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(&mut id, "{prefix}{section_ordinal}{marker}{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(ScopedOmIndexId {
        id,
        _reservation: reservation,
    })
}

fn retained_om_index_id(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    section_ordinal: usize,
    marker: &'static str,
    ordinal: u64,
    operation: &'static str,
) -> Result<String, CodecError> {
    use std::fmt::Write;

    let length = prefix
        .len()
        .checked_add(
            section_ordinal
                .checked_ilog10()
                .map_or(1, |digits| digits as usize + 1),
        )
        .and_then(|length| length.checked_add(marker.len()))
        .and_then(|length| {
            length.checked_add(
                ordinal
                    .checked_ilog10()
                    .map_or(1, |digits| digits as usize + 1),
            )
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(&mut id, "{prefix}{section_ordinal}{marker}{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

fn retained_om_three_number_id(
    ctx: &DecodeContext<'_>,
    segments: [(&str, usize); 3],
    operation: &'static str,
) -> Result<String, CodecError> {
    use std::fmt::Write;

    let digits = |value: usize| value.checked_ilog10().map_or(1, |count| count as usize + 1);
    let length = segments
        .into_iter()
        .try_fold(0usize, |length, (text, number)| {
            length
                .checked_add(text.len())
                .and_then(|length| length.checked_add(digits(number)))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    let [(prefix, first), (middle, second), (suffix, third)] = segments;
    write!(&mut id, "{prefix}{first}{middle}{second}{suffix}{third}")
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

fn copy_om_retained_texts(
    ctx: &DecodeContext<'_>,
    values: &[&str],
    operation: &'static str,
) -> Result<Vec<String>, CodecError> {
    let slot_bytes = values
        .len()
        .checked_mul(std::mem::size_of::<String>())
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(values.len()),
        operation,
    )?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(slot_bytes), operation)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(values.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    for value in values {
        output.push(ctx.copy_retained_text(value, operation)?);
    }
    Ok(output)
}

/// Decode complete zero-prefixed control arrays from offset-only OM stores.
pub(super) fn data_block_control_values(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockControlValue>, CodecError> {
    let mut rows = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(crate::om::OffsetStoreControlForm::ZeroPrefixed { values }) =
            crate::om::offset_store_control_form(
                ctx,
                control.bytes,
                records.first().map(|record| record.bytes),
            )?
        else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let data_block = scoped_om_index_id(
            ctx,
            "nx:om-data-blocks-",
            section_ordinal,
            ":block#",
            0,
            "NX control value block id",
        )?;
        for (ordinal, value) in values.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX data block control value ordinal", 0, 1))?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
                .and_then(|offset| offset.checked_add(u64::from(ordinal).checked_mul(4)?))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("NX data block control value source offset", 0, 1)
                })?;
            let id = retained_om_index_id(
                ctx,
                "nx:om-data-block-control-values-",
                section_ordinal,
                ":value#",
                u64::from(ordinal),
                "retain NX control value id",
            )?;
            let block_reference = ctx.copy_retained_text(&data_block.id, "retain NX control value block reference")?;
            ctx.charge_entities(1, "NX data block control value")?;
            ctx.charge_collection_items(1, "NX data block control values")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<DataBlockControlValue>()),
                "retain NX data block control value",
            )?;
            rows.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX data block control values", 0, 1)
            })?;
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
    let framed_sections = container.om_sections(ctx)?;
    let indexed_sections = container.indexed_om_sections(ctx)?;
    let mut rows = Vec::new();
    for (section_ordinal, (entry, section)) in indexed_sections.iter().enumerate() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        if !matches!(
            crate::om::offset_store_control_form(
                ctx,
                control.bytes,
                records.first().map(|record| record.bytes),
            )?,
            Some(crate::om::OffsetStoreControlForm::ZeroPrefixed { .. })
        ) {
            continue;
        }
        let definition_count = framed_sections
            .iter()
            .filter(|(candidate, _)| candidate.index() == entry.index())
            .try_fold(0usize, |count, (_, section)| {
                count.checked_add(section.types.len())
            })
            .and_then(|count| {
                indexed_sections
                    .iter()
                    .filter(|(candidate, _)| candidate.index() == entry.index())
                    .try_fold(count, |count, (_, section)| {
                        count.checked_add(section.types.len())
                    })
            })
            .ok_or_else(|| ctx.refuse_codec_limit("NX control class registry size", 0, 1))?;
        let registry_node_bytes = std::mem::size_of::<(usize, &crate::om::TypeDefinition<'_>)>()
            .checked_mul(4)
            .ok_or_else(|| ctx.refuse_codec_limit("NX control class registry size", 0, 1))?;
        let registry_bytes = definition_count
            .checked_mul(registry_node_bytes)
            .ok_or_else(|| ctx.refuse_codec_limit("NX control class registry size", 0, 1))?;
        let _registry_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(registry_bytes),
            "NX control class registry",
        )?;
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
            if let std::collections::btree_map::Entry::Vacant(e) = registry.entry(definition.offset)
            {
                ctx.charge_collection_items(1, "NX control class registry entries")?;
                e.insert(definition);
            }
        }
        let Some(ordinals) = crate::om::offset_store_control_class_ordinals(ctx, control.bytes)?
        else {
            continue;
        };
        let entry_index = entry.index();
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (ordinal, class_ordinal) in ordinals.into_iter().enumerate() {
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX control class ordinal", 0, 1))?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(registry.len()),
                "search NX control class registry",
            )?;
            let definition = usize::try_from(class_ordinal)
                .ok()
                .and_then(|ordinal| registry.values().nth(ordinal));
            ctx.charge_collection_items(1, "NX control class references")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    DataBlockControlClassReference,
                >()),
                "retain NX control class references",
            )?;
            rows.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX control class references", 0, 1)
            })?;
            let class = definition
                .map(|definition| -> Result<_, CodecError> {
                    Ok(DataBlockControlClassRef {
                        definition: retained_om_index_id(
                            ctx,
                            "nx:om-entry-",
                            entry_index,
                            ":class#",
                            cadmpeg_core::decode::u64_from_index(definition.offset),
                            "NX control class definition id",
                        )?,
                        name: ctx.copy_retained_text(definition.name, "NX control class name")?,
                    })
                })
                .transpose()?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
                .and_then(|offset| offset.checked_add(u64::from(ordinal_u32).checked_mul(4)?))
                .ok_or_else(|| ctx.refuse_codec_limit("NX control class source offset", 0, 1))?;
            rows.push(DataBlockControlClassReference {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-data-block-control-class-references-",
                    section_ordinal,
                    ":class#",
                    u64::from(ordinal_u32),
                    "NX control class reference id",
                )?,
                data_block: retained_om_index_id(
                    ctx,
                    "nx:om-data-blocks-",
                    section_ordinal,
                    ":block#",
                    0,
                    "NX control class block id",
                )?,
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
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(crate::om::OffsetStoreControlForm::ProductAnchored {
            leading_value,
            values,
        }) = crate::om::offset_store_control_form(
            ctx,
            control.bytes,
            records.first().map(|record| record.bytes),
        )?
        else {
            continue;
        };
        let leading_value_width = leading_value.map_or(0, ControlLeadingValue::width);
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let data_block = scoped_om_index_id(
            ctx,
            "nx:om-data-blocks-",
            section_ordinal,
            ":block#",
            0,
            "NX control index value block id",
        )?;
        let block_count = records
            .len()
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX control index value block count", 0, 1))?;
        for (ordinal, value) in values.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX control index value ordinal", 0, 1))?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(control.offset))
                .and_then(|offset| offset.checked_add(u64::from(leading_value_width)))
                .and_then(|offset| offset.checked_add(u64::from(ordinal).checked_mul(4)?))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("NX control index value source offset", 0, 1)
                })?;
            let id = retained_om_index_id(
                ctx,
                "nx:om-data-block-control-index-values-",
                section_ordinal,
                ":value#",
                u64::from(ordinal),
                "retain NX control index value id",
            )?;
            let block_reference = ctx.copy_retained_text(&data_block.id, "retain NX control index value block reference")?;
            let target_data_block = if usize::try_from(value)
                .ok()
                .is_some_and(|target| target < block_count)
            {
                Some(retained_om_index_id(
                    ctx,
                    "nx:om-data-blocks-",
                    section_ordinal,
                    ":block#",
                    u64::from(value),
                    "retain NX control index value target block",
                )?)
            } else {
                None
            };
            ctx.charge_entities(1, "NX data block control index value")?;
            ctx.charge_collection_items(1, "NX data block control index values")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<DataBlockControlIndexValue>(),
                ),
                "retain NX data block control index value",
            )?;
            rows.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX data block control index values", 0, 1)
            })?;
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
    ctx: &DecodeContext<'_>,
    section_ordinal: usize,
    block_count: usize,
    value: u32,
) -> Result<Option<String>, CodecError> {
    let Some(ordinal) = usize::try_from(value)
        .ok()
        .filter(|ordinal| *ordinal < block_count)
    else {
        return Ok(None);
    };
    retained_om_index_id(
        ctx,
        "nx:om-data-blocks-",
        section_ordinal,
        ":block#",
        cadmpeg_core::decode::u64_from_index(ordinal),
        "NX control index data block",
    )
    .map(Some)
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
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some((control, _, _)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let data_block = scoped_om_index_id(
            ctx,
            "nx:om-data-blocks-",
            section_ordinal,
            ":block#",
            0,
            "NX control reference block id",
        )?;
        for (ordinal, reference) in crate::om::references(ctx, control.bytes, control.offset)?
            .into_iter()
            .enumerate()
        {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX control reference ordinal", 0, 1))?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("NX control reference source offset", 0, 1)
                })?;
            let id = retained_om_index_id(
                ctx,
                "nx:om-data-block-control-references-",
                section_ordinal,
                ":reference#",
                cadmpeg_core::decode::u64_from_index(reference.offset),
                "retain NX control reference id",
            )?;
            let block_reference = ctx.copy_retained_text(&data_block.id, "retain NX control reference block reference")?;
            ctx.charge_entities(1, "NX data block control reference")?;
            ctx.charge_collection_items(1, "NX data block control references")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<DataBlockControlReference>(),
                ),
                "retain NX data block control reference",
            )?;
            output.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX data block control references", 0, 1)
            })?;
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
    let mut temporary = ctx.reserve_scoped(0, "NX control handle pair index")?;
    let mut by_block = BTreeMap::<&str, Vec<(&DataBlockControlReference, u32)>>::new();
    for reference in references {
        let DirectReference::PersistentHandle(handle) = reference.reference else {
            continue;
        };
        let key = reference.data_block.as_str();
        ctx.charge_work(
            u64::from(usize::BITS - by_block.len().leading_zeros()) + 1,
            "index NX control handle pair references",
        )?;
        if !by_block.contains_key(key) {
            ctx.charge_collection_items(1, "NX control handle pair blocks")?;
            temporary.grow(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<(&str, Vec<(&DataBlockControlReference, u32)>)>() + 64,
            ))?;
        }
        let entries = by_block.entry(key).or_default();
        ctx.charge_collection_items(1, "NX control handle pair references")?;
        temporary.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
            &DataBlockControlReference,
            u32,
        )>()))?;
        entries.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX control handle pair references", 0, 1)
        })?;
        entries.push((reference, handle));
    }
    let mut pairs = Vec::new();
    for (data_block, mut block_references) in by_block {
        let count = block_references.len();
        let count_u64 = cadmpeg_core::decode::u64_from_index(count);
        let comparisons = count_u64
            .checked_mul(u64::from(usize::BITS - count.leading_zeros()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("sort NX control handle pair references", 0, 1)
            })?;
        ctx.charge_work(comparisons, "sort NX control handle pair references")?;
        let scratch_bytes = count
            .checked_mul(std::mem::size_of::<(&DataBlockControlReference, u32)>())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("sort NX control handle pair references", 0, 1)
            })?;
        let _sort_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(scratch_bytes),
            "sort NX control handle pair references",
        )?;
        block_references.sort_by_key(|(reference, _)| reference.source_offset);
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
                let id = retained_om_number_id(
                    ctx,
                    "nx:om-data-block-control:handle-pair#",
                    first.source_offset,
                    "retain NX control handle pair id",
                )?;
                let data_block =
                    ctx.copy_retained_text(data_block, "retain NX control handle pair block")?;
                let first_reference = ctx.copy_retained_text(&first.id, "retain NX control handle first reference")?;
                let second_reference = ctx.copy_retained_text(&second.id, "retain NX control handle second reference")?;
                ctx.charge_entities(1, "NX control handle pair")?;
                ctx.charge_collection_items(1, "NX control handle pairs")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                        DataBlockControlHandlePair,
                    >()),
                    "retain NX control handle pair",
                )?;
                pairs.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("allocate NX control handle pairs", 0, 1)
                })?;
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

/// Add one borrowed target to the scoped lookup index.
fn push_data_block_target<'a>(
    ctx: &DecodeContext<'_>,
    index: &mut BTreeMap<(&'a str, u32), Vec<&'a str>>,
    source: &'a str,
    object: u32,
    id: &'a str,
) -> Result<(), CodecError> {
    if !index.contains_key(&(source, object)) {
        ctx.charge_collection_items(1, "NX data block target index keys")?;
    }
    let candidates = index.entry((source, object)).or_default();
    ctx.charge_collection_items(1, "NX data block target index members")?;
    candidates
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX data block target index", 0, 1))?;
    candidates.push(id);
    Ok(())
}

fn data_block_reference_id(
    ctx: &DecodeContext<'_>,
    section: usize,
    block: usize,
    ordinal: usize,
) -> Result<String, CodecError> {
    fn digits(mut value: usize) -> usize {
        let mut count = 1;
        while value >= 10 {
            value /= 10;
            count += 1;
        }
        count
    }
    let operation = "NX data block reference id";
    let length = "nx:om-data-block-references-"
        .len()
        .checked_add(digits(section))
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(digits(block)))
        .and_then(|length| length.checked_add(":reference#".len()))
        .and_then(|length| length.checked_add(digits(ordinal)))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(length), operation)?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(
        &mut id,
        "nx:om-data-block-references-{section}-{block}:reference#{ordinal}"
    )
    .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

/// Decode framed object references from offset-only OM data blocks.
pub(super) fn data_block_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    object_records: &[ObjectRecord],
    expression_declarations: &[ExpressionDeclaration],
) -> Result<Vec<DataBlockReference>, cadmpeg_core::CodecError> {
    let index_items = object_records
        .len()
        .checked_add(expression_declarations.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX data block target index size", 0, 1))?;
    let map_node_bytes = std::mem::size_of::<((&str, u32), Vec<&str>)>()
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<&str>().checked_mul(4)?))
        .ok_or_else(|| ctx.refuse_codec_limit("NX data block target index size", 0, 1))?;
    let index_bytes = index_items
        .checked_mul(map_node_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("NX data block target index size", 0, 1))?;
    let _index_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "NX data block target index",
    )?;
    let mut target_records = BTreeMap::<(&str, u32), Vec<&str>>::new();
    for record in object_records {
        let (object_id, _) = record.object_id;
        push_data_block_target(
            ctx,
            &mut target_records,
            &record.source_entry,
            object_id,
            &record.id,
        )?;
    }
    let mut declarations = BTreeMap::<(&str, u32), Vec<&str>>::new();
    for declaration in expression_declarations {
        push_data_block_target(
            ctx,
            &mut declarations,
            &declaration.source_entry,
            declaration.object_id,
            &declaration.id,
        )?;
    }
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (block_ordinal, block) in std::iter::once(control).chain(records).enumerate() {
            for (ordinal, reference) in crate::om::data_block_object_references(ctx, block.bytes)?
                .into_iter()
                .enumerate()
            {
                let ordinal_u32 = u32::try_from(ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX data block reference ordinal", 0, 1))?;
                let key = (entry.name.as_str(), reference.object_index.value());
                let unique =
                    |candidates: Option<&Vec<&str>>| -> Result<Option<String>, CodecError> {
                        let Some([target]) = candidates.map(Vec::as_slice) else {
                            return Ok(None);
                        };
                        ctx.copy_retained_text(target, "NX data block reference target")
                            .map(Some)
                    };
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(block.offset))
                    .and_then(|offset| {
                        offset.checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                    })
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX data block reference source offset", 0, 1)
                    })?;
                ctx.charge_collection_items(1, "NX data block references")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<DataBlockReference>()),
                    "retain NX data block reference",
                )?;
                output.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("allocate NX data block reference", 0, 1)
                })?;
                output.push(DataBlockReference {
                    id: data_block_reference_id(ctx, section_ordinal, block_ordinal, ordinal)?,
                    data_block: retained_om_index_id(
                        ctx,
                        "nx:om-data-blocks-",
                        section_ordinal,
                        ":block#",
                        cadmpeg_core::decode::u64_from_index(block_ordinal),
                        "NX data block reference block",
                    )?,
                    ordinal: ordinal_u32,
                    object: reference.object_index,
                    target_record: unique(target_records.get(&key))?,
                    target_expression_declaration: unique(declarations.get(&key))?,
                    source_offset,
                });
            }
        }
    }
    Ok(output)
}

/// Decode complete part-local color tables from class-declaring offset stores.
pub(super) fn part_color_tables(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<(Vec<PartColorTable>, Vec<PartColorDefinition>), CodecError> {
    const CLASS_NAME: &str = "UGS::COLOR_table";
    let mut tables = Vec::new();
    let mut definitions = Vec::new();

    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
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
        let parsed_tables = crate::om::color_tables(ctx, storage)?;
        let [table] = parsed_tables.as_slice() else {
            continue;
        };
        let entry_index = entry.index();
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_base = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(storage_offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX part color source base", 0, 1))?;
        let table_id = retained_om_number_id(
            ctx,
            "nx:part-color-tables:table#",
            cadmpeg_core::decode::u64_from_index(section_ordinal),
            "NX part color table id",
        )?;
        let definition_count = cadmpeg_core::decode::u64_from_index(PALETTE_SIZE);
        ctx.charge_collection_items(definition_count, "NX part color definitions")?;
        let definition_bytes = PALETTE_SIZE
            .checked_mul(std::mem::size_of::<PartColorDefinition>())
            .ok_or_else(|| ctx.refuse_codec_limit("NX part color definitions", 0, 1))?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(definition_bytes),
            "retain NX part color definitions",
        )?;
        let _definitions_guard = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(definition_bytes),
            "build NX part color definitions",
        )?;
        let mut parsed_definitions = Vec::new();
        parsed_definitions
            .try_reserve_exact(PALETTE_SIZE)
            .map_err(|_| {
                ctx.refuse_codec_limit("allocate NX part color definitions", 0, definition_count)
            })?;
        let id_slots = PALETTE_SIZE
            .checked_mul(std::mem::size_of::<String>())
            .ok_or_else(|| ctx.refuse_codec_limit("NX part color definition ids", 0, 1))?;
        let _ids_guard = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(id_slots),
            "build NX part color definition ids",
        )?;
        let mut definition_ids = Vec::new();
        definition_ids
            .try_reserve_exact(PALETTE_SIZE)
            .map_err(|_| {
                ctx.refuse_codec_limit("allocate NX part color definition ids", 0, definition_count)
            })?;
        for color_index in PaletteIndex::all() {
            let definition = &table.definitions[usize::from(color_index.value()) - 1];
            let id = retained_om_index_id(
                ctx,
                "nx:part-color-definitions-",
                section_ordinal,
                ":color#",
                u64::from(color_index.value()),
                "NX part color definition id",
            )?;
            definition_ids.push(ctx.copy_retained_text(&id, "NX part color table definition link")?);
            let [a, b, c] = definition.components.map(|(component, offset)| {
                source_base
                    .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                    .map(|offset| (component, offset))
                    .ok_or_else(|| ctx.refuse_codec_limit("NX part color component offset", 0, 1))
            });
            let components = [a?, b?, c?];
            let source_offset = source_base
                .checked_add(cadmpeg_core::decode::u64_from_index(definition.offset))
                .ok_or_else(|| ctx.refuse_codec_limit("NX part color definition offset", 0, 1))?;
            parsed_definitions.push(PartColorDefinition {
                id,
                color_table: ctx.copy_retained_text(&table_id, "NX part color definition table link")?,
                color_index,
                name: ctx.copy_retained_text(definition.name, "NX part color name")?,
                components,
                source_offset,
            });
        }
        let definition_ids: [String; PALETTE_SIZE] = definition_ids
            .try_into()
            .map_err(|_| ctx.refuse_codec_limit("NX part color definition count", 0, 1))?;
        let [a, b, c] = table.background.map(|(component, offset)| {
            source_base
                .checked_add(cadmpeg_core::decode::u64_from_index(offset))
                .map(|offset| (component, offset))
                .ok_or_else(|| ctx.refuse_codec_limit("NX part color background offset", 0, 1))
        });
        let background = [a?, b?, c?];
        let source_offset = source_base
            .checked_add(cadmpeg_core::decode::u64_from_index(table.offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX part color table offset", 0, 1))?;
        ctx.charge_collection_items(1, "NX part color tables")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PartColorTable>()),
            "retain NX part color table",
        )?;
        tables
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX part color tables", 0, 1))?;
        definitions.try_reserve(PALETTE_SIZE).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX part color definitions", 0, definition_count)
        })?;
        definitions.append(&mut parsed_definitions);
        tables.push(PartColorTable {
            id: table_id,
            class_definition: retained_om_index_id(
                ctx,
                "nx:om-entry-",
                entry_index,
                ":class#",
                cadmpeg_core::decode::u64_from_index(class.offset),
                "NX part color class id",
            )?,
            background,
            definitions: definition_ids,
            source_entry: ctx.copy_retained_text(&entry.name, "NX part color source entry")?,
            source_offset,
        });
    }

    Ok((tables, definitions))
}

fn rmfastload_target_object_id(
    ctx: &DecodeContext<'_>,
    object_ids: &[RmFastLoadObjectId],
    target: u32,
) -> Result<Option<String>, CodecError> {
    let Ok(target) = usize::try_from(target) else {
        return Ok(None);
    };
    let Some(object_id) = object_ids.get(target) else {
        return Ok(None);
    };
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(object_id.id.len()),
        "retain NX RMFastLoad target identity",
    )?;
    let mut id = String::new();
    id.try_reserve_exact(object_id.id.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX RMFastLoad target identity", 0, 1))?;
    id.push_str(&object_id.id);
    Ok(Some(id))
}

/// Resolve complete composite column-index tables atomically by section.
pub(super) fn data_block_column_index_tables(
    ctx: &DecodeContext<'_>,
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Result<Vec<DataBlockColumnIndexTable>, CodecError> {
    let index_count = linked_rows
        .len()
        .checked_add(target_rows.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX column index table lookup size", 0, 1))?;
    let node_bytes = std::mem::size_of::<(u32, Vec<&DataBlockLinkedIndexRow>)>()
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<&DataBlockLinkedIndexRow>() * 4))
        .ok_or_else(|| ctx.refuse_codec_limit("NX column index table lookup size", 0, 1))?;
    let lookup_bytes = index_count
        .checked_mul(node_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("NX column index table lookup size", 0, 1))?;
    let _lookup_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(lookup_bytes),
        "NX column index table lookup",
    )?;
    let mut linked_by_section = BTreeMap::<u32, Vec<&DataBlockLinkedIndexRow>>::new();
    for row in linked_rows {
        if !linked_by_section.contains_key(&row.section_ordinal) {
            ctx.charge_collection_items(1, "NX linked row section index")?;
        }
        let rows = linked_by_section.entry(row.section_ordinal).or_default();
        ctx.charge_collection_items(1, "NX linked row section members")?;
        rows.try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX linked row section members", 0, 1))?;
        rows.push(row);
    }
    let mut targets_by_section = BTreeMap::<u32, Vec<&DataBlockTargetIndexRow>>::new();
    for row in target_rows {
        if !targets_by_section.contains_key(&row.section_ordinal) {
            ctx.charge_collection_items(1, "NX target row section index")?;
        }
        let rows = targets_by_section.entry(row.section_ordinal).or_default();
        ctx.charge_collection_items(1, "NX target row section members")?;
        rows.try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX target row section members", 0, 1))?;
        rows.push(row);
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
        let order_count = targets
            .len()
            .checked_add(suffix.len())
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit("NX column index table order", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(order_count),
            "check NX column index table order",
        )?;
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
        let mut target_ids = Vec::new();
        for row in &targets {
            ctx.charge_collection_items(1, "NX column index target rows")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>()),
                "retain NX column index target rows",
            )?;
            target_ids.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX column index target rows", 0, 1)
            })?;
            target_ids.push(ctx.copy_retained_text(&row.id, "NX column index target row id")?);
        }
        let mut suffix_ids = Vec::new();
        for row in suffix {
            ctx.charge_collection_items(1, "NX column index linked rows")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>()),
                "retain NX column index linked rows",
            )?;
            suffix_ids.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX column index linked rows", 0, 1)
            })?;
            suffix_ids.push(ctx.copy_retained_text(&row.id, "NX column index linked row id")?);
        }
        let Ok(rows) = ColumnIndexRows::new(
            opening.frame.target_index().atom.value(),
            target_ids,
            suffix_ids,
        ) else {
            continue;
        };
        ctx.charge_collection_items(1, "NX data block column index tables")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<DataBlockColumnIndexTable>()),
            "retain NX data block column index table",
        )?;
        output.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX data block column index table", 0, 1)
        })?;
        output.push(DataBlockColumnIndexTable {
            id: retained_om_number_id(
                ctx,
                "nx:om-data-block-column-index-tables:table#",
                u64::from(section_ordinal),
                "NX column index table id",
            )?,
            section_ordinal,
            opening_linked_row: ctx.copy_retained_text(&opening.id, "NX column index opening row id")?,
            rows,
            source_entry: ctx.copy_retained_text(&opening.source_entry, "NX column index source entry")?,
            source_offset: opening.frame.offset(),
        });
    }
    Ok(output)
}

/// Decode one product/version header from each indexed NX OM store.
pub(super) fn store_headers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<StoreHeader>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
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
            .map_err(|_| ctx.refuse_codec_limit("NX store header section ordinal", 0, 1))?;
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(version.offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX store header source offset", 0, 1))?;
        ctx.charge_collection_items(1, "NX store headers")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<StoreHeader>()),
            "retain NX store header",
        )?;
        output
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX store headers", 0, 1))?;
        let header = OffsetStoreHeader {
            id: retained_om_number_id(
                ctx,
                "nx:om-store-headers:store#",
                cadmpeg_core::decode::u64_from_index(section_ordinal),
                "NX store header id",
            )?,
            section_ordinal: section_ordinal_u32,
            version: version.value.try_into_owned_for_decode(ctx)?,
            source_entry: ctx.copy_retained_text(&entry.name, "NX store header source entry")?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<StringValue>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (record_ordinal, record) in records.iter().enumerate() {
            for (value_ordinal, value) in record.string_values(ctx)?.into_iter().enumerate() {
                let record_id = retained_om_index_id(
                    ctx,
                    "nx:om-record-directory-",
                    section_ordinal,
                    ":entry#",
                    cadmpeg_core::decode::u64_from_index(record_ordinal),
                    "NX string value record id",
                )?;
                let value_ordinal = u32::try_from(value_ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX string value ordinal", 0, 1))?;
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                    .ok_or_else(|| ctx.refuse_codec_limit("NX string value source offset", 0, 1))?;
                let text =
                    ctx.copy_retained_text(value.value.as_str(), "NX string value text")?;
                let text = PrintableString::new(text)
                    .map_err(|_| ctx.refuse_codec_limit("validate NX string value", 0, 1))?;
                ctx.charge_collection_items(1, "NX native string values")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<StringValue>()),
                    "retain NX native string value",
                )?;
                output.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("allocate NX native string values", 0, 1)
                })?;
                output.push(StringValue {
                    id: retained_om_three_number_id(
                        ctx,
                        [
                            ("nx:om-string-values-", section_ordinal),
                            ("-", record_ordinal),
                            (":value#", value.offset),
                        ],
                        "NX string value id",
                    )?,
                    record: record_id,
                    object_id: record.object_id.0,
                    ordinal: value_ordinal,
                    value: text,
                    source_entry: ctx.copy_retained_text(&entry.name, "NX string value source entry")?,
                    source_offset,
                });
            }
        }
    }
    Ok(output)
}

/// Decode ordered tagged references from bounded NX OM records.
pub(super) fn object_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ObjectReference>, cadmpeg_core::CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (record_ordinal, record) in records.iter().enumerate() {
            for (reference_ordinal, reference) in record
                .references(ctx, records.len())?
                .into_iter()
                .enumerate()
            {
                let record_id = retained_om_index_id(
                    ctx,
                    "nx:om-record-directory-",
                    section_ordinal,
                    ":entry#",
                    cadmpeg_core::decode::u64_from_index(record_ordinal),
                    "NX object reference record id",
                )?;
                let reference_ordinal = u32::try_from(reference_ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX object reference ordinal", 0, 1))?;
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX object reference source offset", 0, 1)
                    })?;
                let reference_value = match reference.value {
                    RecordReference::Direct(value) => RecordReference::Direct(value),
                    RecordReference::RecordOrdinal16 { ordinal, .. } => {
                        RecordReference::RecordOrdinal16 {
                            ordinal,
                            target: retained_om_index_id(
                                ctx,
                                "nx:om-record-directory-",
                                section_ordinal,
                                ":entry#",
                                u64::from(ordinal),
                                "NX object reference target record",
                            )?,
                        }
                    }
                };
                ctx.charge_collection_items(1, "NX native object references")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<ObjectReference>()),
                    "retain NX native object reference",
                )?;
                output.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("allocate NX native object references", 0, 1)
                })?;
                output.push(ObjectReference {
                    id: retained_om_three_number_id(
                        ctx,
                        [
                            ("nx:om-references-", section_ordinal),
                            ("-", record_ordinal),
                            (":reference#", reference.offset),
                        ],
                        "NX object reference id",
                    )?,
                    record: record_id,
                    object_id: record.object_id.0,
                    ordinal: reference_ordinal,
                    reference: reference_value,
                    source_entry: ctx.copy_retained_text(&entry.name, "NX object reference source entry")?,
                    source_offset,
                });
            }
        }
    }
    Ok(output)
}

/// Join maximal two-token adjacent persistent-handle runs within object records.
pub(super) fn object_record_handle_pairs(
    ctx: &DecodeContext<'_>,
    references: &[ObjectReference],
) -> Result<Vec<ObjectRecordHandlePair>, CodecError> {
    let index_bytes = references
        .len()
        .checked_mul(
            std::mem::size_of::<(&str, Vec<(&ObjectReference, u32)>)>()
                + 4 * std::mem::size_of::<(&ObjectReference, u32)>(),
        )
        .ok_or_else(|| ctx.refuse_codec_limit("NX record handle pair index size", 0, 1))?;
    let _index_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "NX record handle pair index",
    )?;
    let mut by_record = BTreeMap::<&str, Vec<(&ObjectReference, u32)>>::new();
    for reference in references {
        let RecordReference::Direct(DirectReference::PersistentHandle(handle)) =
            reference.reference
        else {
            continue;
        };
        if !by_record.contains_key(reference.record.as_str()) {
            ctx.charge_collection_items(1, "NX record handle pair groups")?;
        }
        let group = by_record.entry(reference.record.as_str()).or_default();
        ctx.charge_collection_items(1, "NX record handle pair references")?;
        group.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX record handle pair references", 0, 1)
        })?;
        group.push((reference, handle));
    }
    let mut pairs = Vec::new();
    for (record, mut record_references) in by_record {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(record_references.len()),
            "sort NX record handle references",
        )?;
        record_references.sort_by_key(|(reference, _)| reference.source_offset);
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
                ctx.charge_collection_items(1, "NX record handle pairs")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(
                        std::mem::size_of::<ObjectRecordHandlePair>(),
                    ),
                    "retain NX record handle pair",
                )?;
                pairs
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("allocate NX record handle pairs", 0, 1))?;
                pairs.push(ObjectRecordHandlePair {
                    id: retained_om_number_id(
                        ctx,
                        "nx:om-object-record:handle-pair#",
                        first.source_offset,
                        "NX record handle pair id",
                    )?,
                    record: ctx.copy_retained_text(record, "NX record handle pair record")?,
                    object_id: first.object_id,
                    first_reference: ctx.copy_retained_text(&first.id, "NX first handle reference")?,
                    second_reference: ctx.copy_retained_text(&second.id, "NX second handle reference")?,
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

/// Group persistent-handle occurrences into cross-record identities.
pub(super) fn persistent_handles(
    ctx: &DecodeContext<'_>,
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

    let external_count = external
        .iter()
        .try_fold(0usize, |count, record| {
            count.checked_add(record.handles.serialized().len())
        })
        .ok_or_else(|| ctx.refuse_codec_limit("NX persistent handle index size", 0, 1))?;
    let occurrence_count = references
        .len()
        .checked_add(control_references.len())
        .and_then(|count| count.checked_add(external_count))
        .and_then(|count| count.checked_add(external_tail_pairs.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("NX persistent handle index size", 0, 1))?;
    let index_bytes = occurrence_count
        .checked_mul(std::mem::size_of::<(u32, Group<'_>)>() * 4 + std::mem::size_of::<&str>() * 3)
        .ok_or_else(|| ctx.refuse_codec_limit("NX persistent handle index size", 0, 1))?;
    let _index_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "NX persistent handle index",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(occurrence_count),
        "index NX persistent handles",
    )?;
    let mut groups = BTreeMap::<u32, Group<'_>>::new();
    for reference in references {
        let RecordReference::Direct(DirectReference::PersistentHandle(handle)) =
            reference.reference
        else {
            continue;
        };
        if !groups.contains_key(&handle) {
            ctx.charge_collection_items(1, "NX persistent handle groups")?;
        }
        let group = groups.entry(handle).or_default();
        group.occurrence_count = group
            .occurrence_count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX persistent handle occurrence count", 0, 1))?;
        if group.records.last().copied() != Some(reference.record.as_str())
            && !group.records.contains(&reference.record.as_str())
        {
            ctx.charge_collection_items(1, "NX persistent handle record index")?;
            group.records.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX persistent handle record index", 0, 1)
            })?;
            group.records.push(reference.record.as_str());
        }
    }
    for reference in control_references {
        let DirectReference::PersistentHandle(handle) = reference.reference else {
            continue;
        };
        if !groups.contains_key(&handle) {
            ctx.charge_collection_items(1, "NX persistent handle groups")?;
        }
        let group = groups.entry(handle).or_default();
        group.occurrence_count = group
            .occurrence_count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX persistent handle occurrence count", 0, 1))?;
        if !group.data_blocks.contains(&reference.data_block.as_str()) {
            ctx.charge_collection_items(1, "NX persistent handle data block index")?;
            group.data_blocks.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX persistent handle data block index", 0, 1)
            })?;
            group.data_blocks.push(reference.data_block.as_str());
        }
    }
    for record in external {
        for handle in record.handles.serialized() {
            if !groups.contains_key(handle) {
                ctx.charge_collection_items(1, "NX persistent handle groups")?;
            }
            let group = groups.entry(*handle).or_default();
            group.external_occurrence_count = group
                .external_occurrence_count
                .checked_add(1)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("NX external handle occurrence count", 0, 1)
                })?;
            if !group.external_records.contains(&record.id.as_str()) {
                ctx.charge_collection_items(1, "NX persistent external record index")?;
                group.external_records.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("allocate NX persistent external record index", 0, 1)
                })?;
                group.external_records.push(record.id.as_str());
            }
        }
    }
    for pair in external_tail_pairs {
        if !groups.contains_key(&pair.persistent_handle) {
            ctx.charge_collection_items(1, "NX persistent handle groups")?;
        }
        let group = groups.entry(pair.persistent_handle).or_default();
        group.external_occurrence_count = group
            .external_occurrence_count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX external handle occurrence count", 0, 1))?;
        if !group
            .external_records
            .contains(&pair.handle_set_record.as_str())
        {
            ctx.charge_collection_items(1, "NX persistent external record index")?;
            group.external_records.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX persistent external record index", 0, 1)
            })?;
            group.external_records.push(pair.handle_set_record.as_str());
        }
    }
    let mut handles = Vec::new();
    for (value, group) in groups {
        use std::fmt::Write;
        ctx.charge_collection_items(1, "NX persistent handles")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PersistentHandle>()),
            "retain NX persistent handle",
        )?;
        handles
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX persistent handles", 0, 1))?;
        let prefix = "nx:om-persistent-handles:handle#";
        let id_length = prefix
            .len()
            .checked_add(8)
            .ok_or_else(|| ctx.refuse_codec_limit("NX persistent handle id", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(id_length),
            "NX persistent handle id",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_length),
            "NX persistent handle id",
        )?;
        let mut id = String::new();
        id.try_reserve_exact(id_length)
            .map_err(|_| ctx.refuse_codec_limit("NX persistent handle id", 0, 1))?;
        write!(&mut id, "{prefix}{value:08x}")
            .map_err(|_| ctx.refuse_codec_limit("NX persistent handle id", 0, 1))?;
        handles.push(PersistentHandle {
            id,
            value,
            records: copy_om_retained_texts(ctx, &group.records, "NX persistent handle records")?,
            occurrence_count: group.occurrence_count,
            data_blocks: copy_om_retained_texts(
                ctx,
                &group.data_blocks,
                "NX persistent handle data blocks",
            )?,
            external_records: copy_om_retained_texts(
                ctx,
                &group.external_records,
                "NX persistent handle external records",
            )?,
            external_occurrence_count: group.external_occurrence_count,
        });
    }
    Ok(handles)
}

/// Decode named parameter declarations from expression-class OM records.
pub(super) fn expression_declarations(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ExpressionDeclaration>, CodecError> {
    let mut declarations = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
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
            let Some(declaration) = crate::om::expression_declaration_name(ctx, record.bytes)?
            else {
                continue;
            };
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(record.offset))
                .and_then(|offset| {
                    offset.checked_add(cadmpeg_core::decode::u64_from_index(declaration.offset))
                })
                .ok_or_else(|| ctx.refuse_codec_limit("NX declaration source offset", 0, 1))?;
            let name =
                ctx.copy_retained_text(declaration.name.as_str(), "NX declaration name")?;
            let name = ParameterName::<String, u32>::parse(name)
                .ok_or_else(|| ctx.refuse_codec_limit("validate NX declaration name", 0, 1))?;
            let literal = declaration
                .literal
                .map(|literal| ctx.copy_retained_text(literal, "NX declaration literal"))
                .transpose()?;
            ctx.charge_collection_items(1, "NX expression declarations")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<ExpressionDeclaration>()),
                "retain NX expression declaration",
            )?;
            declarations
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("allocate NX expression declarations", 0, 1))?;
            declarations.push(ExpressionDeclaration {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-expression-declarations-",
                    section_ordinal,
                    ":declaration#",
                    cadmpeg_core::decode::u64_from_index(record_ordinal),
                    "NX declaration id",
                )?,
                object_id: record.object_id.0,
                record: retained_om_index_id(
                    ctx,
                    "nx:om-record-directory-",
                    section_ordinal,
                    ":entry#",
                    cadmpeg_core::decode::u64_from_index(record_ordinal),
                    "NX declaration record id",
                )?,
                name,
                literal,
                source_entry: ctx.copy_retained_text(&entry.name, "NX declaration source entry")?,
                source_offset,
            });
        }
    }
    Ok(declarations)
}

/// Decode explicit numeric expressions from all indexed OM sections.
pub(super) fn expressions(
    ctx: &DecodeContext<'_>,
    container: &Container,
    declarations: &[ExpressionDeclaration],
) -> Result<Vec<Expression>, CodecError> {
    let declaration_bytes = declarations
        .len()
        .checked_mul(
            std::mem::size_of::<((&str, &str), Vec<&ExpressionDeclaration>)>() * 4
                + std::mem::size_of::<&ExpressionDeclaration>() * 4,
        )
        .ok_or_else(|| ctx.refuse_codec_limit("NX expression declaration index size", 0, 1))?;
    let _declaration_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(declaration_bytes),
        "NX expression declaration index",
    )?;
    let mut declarations_by_name = BTreeMap::<(&str, &str), Vec<&ExpressionDeclaration>>::new();
    for declaration in declarations {
        let key = (declaration.source_entry.as_str(), declaration.name.as_str());
        if !declarations_by_name.contains_key(&key) {
            ctx.charge_collection_items(1, "NX declaration name groups")?;
        }
        let group = declarations_by_name.entry(key).or_default();
        ctx.charge_collection_items(1, "NX declaration name members")?;
        group
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("allocate NX declaration name members", 0, 1))?;
        group.push(declaration);
    }
    let sections = container.indexed_om_sections(ctx)?;
    let maximum_indexed = sections
        .iter()
        .try_fold(0usize, |count, (_, section)| {
            let records = match &section.store {
                IndexedStore::Fixed { records } => records.len(),
                IndexedStore::OffsetOnly { records, .. } => records.len(),
            };
            count.checked_add(records)
        })
        .ok_or_else(|| ctx.refuse_codec_limit("NX indexed expression lookup size", 0, 1))?;
    let indexed_bytes = maximum_indexed
        .checked_mul(std::mem::size_of::<((&str, usize), (u32, usize, usize))>() * 4)
        .ok_or_else(|| ctx.refuse_codec_limit("NX indexed expression lookup size", 0, 1))?;
    let _indexed_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(indexed_bytes),
        "NX indexed expression lookup",
    )?;
    let mut indexed = BTreeMap::<(&str, usize), (u32, usize, usize)>::new();
    for (section_ordinal, (entry, section)) in sections.into_iter().enumerate() {
        let Some(directory_entry) = container.entries.get(entry.index()) else {
            continue;
        };
        for (record_ordinal, expression) in section.numeric_expression_records(ctx)? {
            let Some(object_id) = expression.object_id else {
                continue;
            };
            if !indexed.contains_key(&(directory_entry.name.as_str(), expression.offset)) {
                ctx.charge_collection_items(1, "NX indexed expression lookup entries")?;
            }
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
        for expression in crate::om::numeric_expressions(ctx, payload)? {
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
            let value = expression.constant_value(ctx)?;
            let source_table_text = retained_om_index_id(
                ctx,
                "nx:om-entry-",
                entry_index,
                ":expression-table#",
                cadmpeg_core::decode::u64_from_index(table_offset),
                "NX expression source table",
            )?;
            let Some(source_table) = cadmpeg_core::text::NonBlankString::new(source_table_text)
            else {
                continue;
            };
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(expression.offset))
                .ok_or_else(|| ctx.refuse_codec_limit("NX expression source offset", 0, 1))?;
            ctx.charge_collection_items(1, "NX native expressions")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Expression>()),
                "retain NX native expression",
            )?;
            expressions
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("allocate NX native expressions", 0, 1))?;
            expressions.push(Expression {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-entry-",
                    entry_index,
                    ":expression#",
                    cadmpeg_core::decode::u64_from_index(expression.offset),
                    "NX expression id",
                )?,
                owner: indexed_record
                    .map(|(object_id, section_ordinal, record_ordinal)| {
                        retained_om_index_id(
                            ctx,
                            "nx:om-record-directory-",
                            section_ordinal,
                            ":entry#",
                            cadmpeg_core::decode::u64_from_index(record_ordinal),
                            "NX expression owner record",
                        )
                        .map(|record| ExpressionOwner { object_id, record })
                    })
                    .transpose()?,
                declaration: declaration
                    .map(|declaration| {
                        ctx.copy_retained_text(&declaration.id, "NX expression declaration id")
                    })
                    .transpose()?,
                name: ParameterName::new(ctx.copy_retained_text(expression.name.as_str(), "NX expression name")?),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Inch => ExpressionUnit::Inch,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                    crate::om::ExpressionUnit::Native(unit) => ExpressionUnit::Native(unit),
                },
                expression: ctx.copy_retained_text(expression.expression, "NX expression formula")?,
                value,
                source_entry: ctx.copy_retained_text(&entry.name, "NX expression source entry")?,
                source_table,
                source_offset,
            });
        }
    }
    evaluate_expression_graphs(ctx, &mut expressions)?;
    Ok(expressions)
}

fn evaluate_expression_graphs(
    ctx: &DecodeContext<'_>,
    expressions: &mut [Expression],
) -> Result<(), CodecError> {
    struct Group {
        count: usize,
        value: Option<FiniteReal>,
    }

    let index_bytes = expressions
        .len()
        .checked_mul(std::mem::size_of::<((&str, &str, &ExpressionUnit), Group)>() * 4)
        .ok_or_else(|| ctx.refuse_codec_limit("NX expression graph index size", 0, 1))?;
    let _index_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(index_bytes),
        "NX expression graph index",
    )?;
    let mut groups = BTreeMap::<(&str, &str, &ExpressionUnit), Group>::new();
    for expression in expressions.iter() {
        let key = (
            expression.source_table.as_str(),
            expression.name.as_str(),
            &expression.unit,
        );
        match groups.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "NX expression graph names")?;
                entry.insert(Group {
                    count: 1,
                    value: expression.value,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let group = entry.get_mut();
                group.count = group.count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("NX expression graph name count", 0, 1)
                })?;
                group.value = None;
            }
        }
    }
    loop {
        let mut changed = false;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(expressions.len()),
            "evaluate NX expression graph",
        )?;
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
            let evaluated =
                evaluate_parameterized_expression(ctx, &expression.expression, |name| {
                    let dependency = (expression.source_table.as_str(), name, &expression.unit);
                    groups
                        .get(&dependency)
                        .filter(|group| group.count == 1)
                        .and_then(|group| group.value)
                        .map(FiniteReal::get)
                })?;
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
    let result_bytes = expressions
        .len()
        .checked_mul(std::mem::size_of::<Option<FiniteReal>>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX expression graph results size", 0, 1))?;
    let _result_guard = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(result_bytes),
        "NX expression graph results",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(expressions.len()),
        "NX expression graph results",
    )?;
    let mut results = Vec::new();
    results
        .try_reserve_exact(expressions.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX expression graph results", 0, 1))?;
    for expression in expressions.iter() {
        let key = (
            expression.source_table.as_str(),
            expression.name.as_str(),
            &expression.unit,
        );
        results.push(
            groups
                .get(&key)
                .filter(|group| group.count == 1)
                .and_then(|group| group.value),
        );
    }
    drop(groups);
    for (expression, value) in expressions.iter_mut().zip(results) {
        expression.value = value;
    }
    Ok(())
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
        let identity = crate::test_support::with_decode_context(|ctx| {
            super::stable_object_record_identity(ctx, "/Root/UG_PART/UG_PART", &bytes)
        })
        .unwrap();
        assert_eq!(
            identity,
            crate::test_support::with_decode_context(|ctx| {
                super::stable_object_record_identity(ctx, "/Root/UG_PART/UG_PART", &bytes)
            })
            .unwrap()
        );
        assert_ne!(
            identity,
            crate::test_support::with_decode_context(|ctx| {
                super::stable_object_record_identity(ctx, "/Root/other", &bytes)
            })
            .unwrap()
        );
        assert_ne!(
            identity,
            crate::test_support::with_decode_context(|ctx| {
                super::stable_object_record_identity(
                    ctx,
                    "/Root/UG_PART/UG_PART",
                    &[0x04, 0x05, 0x07],
                )
            })
            .unwrap()
        );
    }

    #[test]
    fn unique_indexed_object_records_receive_stable_identities() {
        let container = crate::test_support::with_decode_context(|ctx| {
            crate::container::scan_bytes(ctx, prt_with_indexed_om_section())
        })
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

    fn object_record_identity_limit_error(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let file = prt_with_indexed_om_section();
        let scan_arena = DecodeArena::new();
        let scan_policy = DecodePolicy::service();
        let (scan_ctx, _) =
            DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy).unwrap();
        let container = crate::container::scan_bytes(&scan_ctx, &file).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        configure(&mut policy);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::object_records(&ctx, &container).unwrap_err()
    }

    #[test]
    fn object_record_identity_route_refuses_work_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let error = object_record_identity_limit_error(|policy| policy.limits.max_work_units = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits)
        );
    }

    #[test]
    fn object_record_identity_route_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let error =
            object_record_identity_limit_error(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn object_record_identity_route_refuses_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let error =
            object_record_identity_limit_error(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn object_record_identity_route_refuses_scoped_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let error =
            object_record_identity_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes)
        );
    }

    fn stable_identities_for_test(records: &[&[u8]]) -> Vec<Option<String>> {
        crate::test_support::with_decode_context(|ctx| {
            super::stable_object_record_identities(ctx, "/entry", records)
        })
        .unwrap()
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
    [u8; 8],
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
