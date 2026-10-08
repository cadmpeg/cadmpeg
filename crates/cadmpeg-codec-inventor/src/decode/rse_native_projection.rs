// SPDX-License-Identifier: Apache-2.0
//! Native records projected from `RSe` segment inventory.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::container::InventorContainer;
use crate::native::{
    MetaSectionRecord, MetaTypeRecord, RseRecordRecord, SegmentBulkFrame, SegmentBulkIssueRecord,
    SegmentBulkRecord, SegmentMetaIssueRecord, SegmentMetaRecord, SegmentPairRecord,
    StructuralIssueRecord, UnpairedMember, UnpairedSegmentRecord,
};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentMetaState};

use super::retained_hex;

pub(super) struct RseNativeProjection {
    pub(super) identity_issues: Vec<StructuralIssueRecord>,
    pub(super) segment_pairs: Vec<SegmentPairRecord>,
    pub(super) segment_meta: Vec<SegmentMetaRecord>,
    pub(super) meta_sections: Vec<MetaSectionRecord>,
    pub(super) meta_types: Vec<MetaTypeRecord>,
    pub(super) segment_meta_issues: Vec<SegmentMetaIssueRecord>,
    pub(super) rse_records: Vec<RseRecordRecord>,
    pub(super) segment_bulk: Vec<SegmentBulkRecord>,
    pub(super) segment_bulk_issues: Vec<SegmentBulkIssueRecord>,
    pub(super) unpaired_segments: Vec<UnpairedSegmentRecord>,
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    container: &InventorContainer<'_>,
) -> Result<RseNativeProjection, CodecError> {
    let mut projection = RseNativeProjection {
        identity_issues: Vec::new(),
        segment_pairs: Vec::new(),
        segment_meta: Vec::new(),
        meta_sections: Vec::new(),
        meta_types: Vec::new(),
        segment_meta_issues: Vec::new(),
        rse_records: Vec::new(),
        segment_bulk: Vec::new(),
        segment_bulk_issues: Vec::new(),
        unpaired_segments: Vec::new(),
    };
    let mut segment_steps = container.rse.segments.iter();
    while let Some(segment) = ctx.next_charged(
        &mut segment_steps,
        "visit Inventor decode/rse_native_projection items",
    )? {
        let token = segment.pair.token.as_str();
        let mut issue_steps = segment.identity_issues.iter().enumerate();
        while let Some((ordinal, detail)) = ctx.next_charged(
            &mut issue_steps,
            "visit Inventor decode/rse_native_projection items",
        )? {
            ctx.charge_entities(1, "admit Inventor native structural records")?;
            ctx.push_vec(
                &mut projection.identity_issues,
                StructuralIssueRecord {
                    id: ctx.format_retained(
                        format_args!("inventor:rse:structural-issue#segment-{token}-{ordinal}"),
                        "retain Inventor segment identity issue id",
                    )?,
                    scope: ctx.format_retained(
                        format_args!("segment:{token}"),
                        "retain Inventor segment identity issue scope",
                    )?,
                    detail: ctx.copy_retained_text(
                        detail,
                        "retain Inventor segment identity issue detail",
                    )?,
                },
                "retain Inventor native structural records",
            )?;
        }
        ctx.charge_entities(1, "admit Inventor native structural records")?;
        ctx.push_vec(
            &mut projection.segment_pairs,
            SegmentPairRecord {
                id: ctx.format_retained(
                    format_args!("inventor:rse:segment#{token}"),
                    "retain Inventor segment pair id",
                )?,
                token: ctx.copy_retained_text(token, "retain Inventor segment pair token")?,
                metadata_directory_id: segment.pair.metadata.directory_id(),
                bulk_directory_id: segment.pair.bulk.directory_id(),
            },
            "retain Inventor native structural records",
        )?;
        match &segment.meta {
            SegmentMetaState::Parsed(meta) => {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                ctx.push_vec(
                    &mut projection.segment_meta,
                    SegmentMetaRecord {
                        id: ctx.format_retained(
                            format_args!("inventor:rse:segment-meta#{token}"),
                            "retain Inventor segment metadata id",
                        )?,
                        token: ctx
                            .copy_retained_text(token, "retain Inventor segment metadata token")?,
                        version: meta.declared.version,
                        kind: segment.kind.retained_label(ctx)?,
                        display_name: ctx.copy_retained_text(
                            &meta.display_name,
                            "retain Inventor segment display name",
                        )?,
                        segment_id: retained_hex(
                            ctx,
                            &meta.segment_id,
                            "retain Inventor segment GUID",
                        )?,
                        header_values: meta.header_values,
                        state_words: meta.state_words,
                        created: ctx.copy_retained_text(
                            &meta.created,
                            "retain Inventor segment creation text",
                        )?,
                        modified: ctx.copy_retained_text(
                            &meta.modified,
                            "retain Inventor segment modification text",
                        )?,
                        body_form: meta.body_form,
                        expanded_body_len: cadmpeg_core::decode::u64_from_index(
                            meta.body.window().len(),
                        ),
                        expanded_body_sha256:
                            cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                                ctx,
                                meta.body.window(),
                                "retain Inventor segment body digest",
                            )
                            .map(String::from)?,
                        table_prefix: meta.tables.prefix,
                        block_count: cadmpeg_core::decode::u64_from_index(meta.tables.blocks.len()),
                        type_count: cadmpeg_core::decode::u64_from_index(meta.tables.types.len()),
                        terminal_id: retained_hex(
                            ctx,
                            &meta.tables.terminal_id,
                            "retain Inventor segment terminal GUID",
                        )?,
                    },
                    "retain Inventor native structural records",
                )?;
                // The metadata tables always hold their eleven fixed sections.
                for section in &meta.tables.sections {
                    ctx.charge_entities(1, "admit Inventor native structural records")?;
                    ctx.push_vec(
                        &mut projection.meta_sections,
                        MetaSectionRecord {
                            id: ctx.format_retained(
                                format_args!(
                                    "inventor:rse:meta-section#{token}-{}",
                                    section.number
                                ),
                                "retain Inventor metadata section id",
                            )?,
                            token: ctx.copy_retained_text(
                                token,
                                "retain Inventor metadata section token",
                            )?,
                            number: section.number,
                            discriminator: section.discriminator,
                            payload_len: cadmpeg_core::decode::u64_from_index(
                                section.payload.window().len(),
                            ),
                            payload_sha256:
                                cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                                    ctx,
                                    section.payload.window(),
                                    "retain Inventor metadata section digest",
                                )
                                .map(String::from)?,
                        },
                        "retain Inventor native structural records",
                    )?;
                }
                let mut type_steps = meta.tables.types.iter();
                while let Some(descriptor) = ctx.next_charged(
                    &mut type_steps,
                    "visit Inventor decode/rse_native_projection items",
                )? {
                    ctx.charge_entities(1, "admit Inventor native structural records")?;
                    ctx.push_vec(
                        &mut projection.meta_types,
                        MetaTypeRecord {
                            id: ctx.format_retained(
                                format_args!("inventor:rse:meta-type#{token}-{}", descriptor.index),
                                "retain Inventor metadata type id",
                            )?,
                            token: ctx
                                .copy_retained_text(token, "retain Inventor metadata type token")?,
                            index: descriptor.index,
                            type_id: retained_hex(
                                ctx,
                                &descriptor.id,
                                "retain Inventor metadata type GUID",
                            )?,
                            fields: descriptor.fields,
                        },
                        "retain Inventor native structural records",
                    )?;
                }
            }
            SegmentMetaState::Malformed { detail, .. } => {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                ctx.push_vec(
                    &mut projection.segment_meta_issues,
                    SegmentMetaIssueRecord {
                        id: ctx.format_retained(
                            format_args!("inventor:rse:segment-meta-issue#{token}"),
                            "retain Inventor metadata issue id",
                        )?,
                        token: ctx
                            .copy_retained_text(token, "retain Inventor metadata issue token")?,
                        detail: ctx
                            .copy_retained_text(detail, "retain Inventor metadata issue detail")?,
                    },
                    "retain Inventor native structural records",
                )?;
            }
        }
        match &segment.bulk {
            SegmentBulkState::Framed(bulk) => {
                let records = match &bulk.records {
                    RecordFrameState::Framed(table) => {
                        let mut record_steps = table.records.iter();
                        while let Some(record) = ctx.next_charged(
                            &mut record_steps,
                            "visit Inventor decode/rse_native_projection items",
                        )? {
                            ctx.charge_entities(1, "admit Inventor native structural records")?;
                            ctx.push_vec(
                                &mut projection.rse_records,
                                RseRecordRecord::from_frame(ctx, token, record)?,
                                "retain Inventor native structural records",
                            )?;
                        }
                        SegmentBulkFrame::Framed {
                            record_count: cadmpeg_core::decode::u64_from_index(table.records.len()),
                            stream_trailer_len: cadmpeg_core::decode::u64_from_index(
                                table.stream_trailer.window().len(),
                            ),
                            stream_trailer_sha256:
                                cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                                    ctx,
                                    table.stream_trailer.window(),
                                    "retain Inventor RSe stream trailer digest",
                                )
                                .map(String::from)?,
                        }
                    }
                    RecordFrameState::Unavailable(detail) => SegmentBulkFrame::Unavailable {
                        detail: ctx
                            .copy_retained_text(detail, "retain Inventor RSe frame issue")?,
                    },
                };
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                ctx.push_vec(
                    &mut projection.segment_bulk,
                    SegmentBulkRecord {
                        id: ctx.format_retained(
                            format_args!("inventor:rse:segment-bulk#{token}"),
                            "retain Inventor segment bulk id",
                        )?,
                        token: ctx
                            .copy_retained_text(token, "retain Inventor segment bulk token")?,
                        prefix: retained_hex(
                            ctx,
                            &bulk.prefix,
                            "retain Inventor segment bulk prefix",
                        )?,
                        form: bulk.form.value(),
                        compressed_len: cadmpeg_core::decode::u64_from_index(
                            bulk.compressed.window().len(),
                        ),
                        compressed_sha256:
                            cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                                ctx,
                                bulk.compressed.window(),
                                "retain Inventor compressed bulk digest",
                            )
                            .map(String::from)?,
                        expanded_len: cadmpeg_core::decode::u64_from_index(
                            bulk.expanded.window().len(),
                        ),
                        expanded_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                            ctx,
                            bulk.expanded.window(),
                            "retain Inventor expanded bulk digest",
                        )
                        .map(String::from)?,
                        records,
                    },
                    "retain Inventor native structural records",
                )?;
            }
            SegmentBulkState::Malformed(detail) => {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                ctx.push_vec(
                    &mut projection.segment_bulk_issues,
                    SegmentBulkIssueRecord {
                        id: ctx.format_retained(
                            format_args!("inventor:rse:segment-bulk-issue#{token}"),
                            "retain Inventor bulk issue id",
                        )?,
                        token: ctx.copy_retained_text(token, "retain Inventor bulk issue token")?,
                        detail: ctx
                            .copy_retained_text(detail, "retain Inventor bulk issue detail")?,
                    },
                    "retain Inventor native structural records",
                )?;
            }
        }
    }
    let mut token_steps = container.rse.unpaired_metadata.iter();
    while let Some(token) = ctx.next_charged(
        &mut token_steps,
        "visit Inventor decode/rse_native_projection items",
    )? {
        ctx.charge_entities(1, "admit Inventor native structural records")?;
        ctx.push_vec(
            &mut projection.unpaired_segments,
            UnpairedSegmentRecord {
                id: ctx.format_retained(
                    format_args!("inventor:rse:unpaired-metadata#{}", token.as_str()),
                    "retain Inventor unpaired metadata id",
                )?,
                token: ctx.copy_retained_text(
                    token.as_str(),
                    "retain Inventor unpaired metadata token",
                )?,
                missing_member: UnpairedMember::Bulk,
            },
            "retain Inventor native structural records",
        )?;
    }
    let mut token_steps = container.rse.unpaired_bulk.iter();
    while let Some(token) = ctx.next_charged(
        &mut token_steps,
        "visit Inventor decode/rse_native_projection items",
    )? {
        ctx.charge_entities(1, "admit Inventor native structural records")?;
        ctx.push_vec(
            &mut projection.unpaired_segments,
            UnpairedSegmentRecord {
                id: ctx.format_retained(
                    format_args!("inventor:rse:unpaired-bulk#{}", token.as_str()),
                    "retain Inventor unpaired bulk id",
                )?,
                token: ctx
                    .copy_retained_text(token.as_str(), "retain Inventor unpaired bulk token")?,
                missing_member: UnpairedMember::Metadata,
            },
            "retain Inventor native structural records",
        )?;
    }
    Ok(projection)
}

#[cfg(test)]
mod tests;
