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

use super::{retained_clone, retained_format, retained_hex, retained_sha256, wire_len};

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
    for segment in &container.rse.segments {
        let token = segment.pair.token.as_str();
        for (ordinal, detail) in segment.identity_issues.iter().enumerate() {
            projection.identity_issues.push(StructuralIssueRecord {
                id: retained_format(
                    ctx,
                    format_args!("inventor:rse:structural-issue#segment-{token}-{ordinal}"),
                    "retain Inventor segment identity issue id",
                )?,
                scope: retained_format(
                    ctx,
                    format_args!("segment:{token}"),
                    "retain Inventor segment identity issue scope",
                )?,
                detail: retained_clone(
                    ctx,
                    detail,
                    "retain Inventor segment identity issue detail",
                )?,
            });
        }
        projection.segment_pairs.push(SegmentPairRecord {
            id: retained_format(
                ctx,
                format_args!("inventor:rse:segment#{token}"),
                "retain Inventor segment pair id",
            )?,
            token: retained_clone(ctx, token, "retain Inventor segment pair token")?,
            metadata_directory_id: segment.pair.metadata.directory_id(),
            bulk_directory_id: segment.pair.bulk.directory_id(),
        });
        match &segment.meta {
            SegmentMetaState::Parsed(meta) => {
                projection.segment_meta.push(SegmentMetaRecord {
                    id: retained_format(
                        ctx,
                        format_args!("inventor:rse:segment-meta#{token}"),
                        "retain Inventor segment metadata id",
                    )?,
                    token: retained_clone(ctx, token, "retain Inventor segment metadata token")?,
                    version: meta.declared.version,
                    kind: retained_clone(
                        ctx,
                        segment.kind.label(),
                        "retain Inventor segment kind",
                    )?,
                    display_name: retained_clone(
                        ctx,
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
                    created: retained_clone(
                        ctx,
                        &meta.created,
                        "retain Inventor segment creation text",
                    )?,
                    modified: retained_clone(
                        ctx,
                        &meta.modified,
                        "retain Inventor segment modification text",
                    )?,
                    body_form: meta.body_form,
                    expanded_body_len: wire_len(
                        ctx,
                        meta.body.window().len(),
                        "Inventor expanded metadata body length",
                    )?,
                    expanded_body_sha256: retained_sha256(
                        ctx,
                        meta.body.window(),
                        "retain Inventor segment body digest",
                    )?,
                    table_prefix: meta.tables.prefix,
                    block_count: wire_len(
                        ctx,
                        meta.tables.blocks.len(),
                        "Inventor metadata block count",
                    )?,
                    type_count: wire_len(
                        ctx,
                        meta.tables.types.len(),
                        "Inventor metadata type count",
                    )?,
                    terminal_id: retained_hex(
                        ctx,
                        &meta.tables.terminal_id,
                        "retain Inventor segment terminal GUID",
                    )?,
                });
                for section in &meta.tables.sections {
                    projection.meta_sections.push(MetaSectionRecord {
                        id: retained_format(
                            ctx,
                            format_args!("inventor:rse:meta-section#{token}-{}", section.number),
                            "retain Inventor metadata section id",
                        )?,
                        token: retained_clone(
                            ctx,
                            token,
                            "retain Inventor metadata section token",
                        )?,
                        number: section.number,
                        discriminator: section.discriminator,
                        payload_len: wire_len(
                            ctx,
                            section.payload.window().len(),
                            "Inventor metadata section payload length",
                        )?,
                        payload_sha256: retained_sha256(
                            ctx,
                            section.payload.window(),
                            "retain Inventor metadata section digest",
                        )?,
                    });
                }
                for descriptor in &meta.tables.types {
                    projection.meta_types.push(MetaTypeRecord {
                        id: retained_format(
                            ctx,
                            format_args!("inventor:rse:meta-type#{token}-{}", descriptor.index),
                            "retain Inventor metadata type id",
                        )?,
                        token: retained_clone(ctx, token, "retain Inventor metadata type token")?,
                        index: descriptor.index,
                        type_id: retained_hex(
                            ctx,
                            &descriptor.id,
                            "retain Inventor metadata type GUID",
                        )?,
                        fields: descriptor.fields,
                    });
                }
            }
            SegmentMetaState::Malformed { detail, .. } => {
                projection.segment_meta_issues.push(SegmentMetaIssueRecord {
                    id: retained_format(
                        ctx,
                        format_args!("inventor:rse:segment-meta-issue#{token}"),
                        "retain Inventor metadata issue id",
                    )?,
                    token: retained_clone(ctx, token, "retain Inventor metadata issue token")?,
                    detail: retained_clone(ctx, detail, "retain Inventor metadata issue detail")?,
                });
            }
        }
        match &segment.bulk {
            SegmentBulkState::Framed(bulk) => {
                let records = match &bulk.records {
                    RecordFrameState::Framed(table) => {
                        for record in &table.records {
                            projection
                                .rse_records
                                .push(RseRecordRecord::from_frame(ctx, token, record)?);
                        }
                        SegmentBulkFrame::Framed {
                            record_count: wire_len(
                                ctx,
                                table.records.len(),
                                "Inventor RSe record count",
                            )?,
                            stream_trailer_len: wire_len(
                                ctx,
                                table.stream_trailer.window().len(),
                                "Inventor RSe stream trailer length",
                            )?,
                            stream_trailer_sha256: retained_sha256(
                                ctx,
                                table.stream_trailer.window(),
                                "retain Inventor RSe stream trailer digest",
                            )?,
                        }
                    }
                    RecordFrameState::Unavailable(detail) => SegmentBulkFrame::Unavailable {
                        detail: retained_clone(ctx, detail, "retain Inventor RSe frame issue")?,
                    },
                };
                projection.segment_bulk.push(SegmentBulkRecord {
                    id: retained_format(
                        ctx,
                        format_args!("inventor:rse:segment-bulk#{token}"),
                        "retain Inventor segment bulk id",
                    )?,
                    token: retained_clone(ctx, token, "retain Inventor segment bulk token")?,
                    prefix: retained_hex(ctx, &bulk.prefix, "retain Inventor segment bulk prefix")?,
                    form: bulk.form.value(),
                    compressed_len: wire_len(
                        ctx,
                        bulk.compressed.window().len(),
                        "Inventor compressed bulk length",
                    )?,
                    compressed_sha256: retained_sha256(
                        ctx,
                        bulk.compressed.window(),
                        "retain Inventor compressed bulk digest",
                    )?,
                    expanded_len: wire_len(
                        ctx,
                        bulk.expanded.window().len(),
                        "Inventor expanded bulk length",
                    )?,
                    expanded_sha256: retained_sha256(
                        ctx,
                        bulk.expanded.window(),
                        "retain Inventor expanded bulk digest",
                    )?,
                    records,
                });
            }
            SegmentBulkState::Malformed(detail) => {
                projection.segment_bulk_issues.push(SegmentBulkIssueRecord {
                    id: retained_format(
                        ctx,
                        format_args!("inventor:rse:segment-bulk-issue#{token}"),
                        "retain Inventor bulk issue id",
                    )?,
                    token: retained_clone(ctx, token, "retain Inventor bulk issue token")?,
                    detail: retained_clone(ctx, detail, "retain Inventor bulk issue detail")?,
                });
            }
        }
    }
    for token in &container.rse.unpaired_metadata {
        projection.unpaired_segments.push(UnpairedSegmentRecord {
            id: retained_format(
                ctx,
                format_args!("inventor:rse:unpaired-metadata#{}", token.as_str()),
                "retain Inventor unpaired metadata id",
            )?,
            token: retained_clone(
                ctx,
                token.as_str(),
                "retain Inventor unpaired metadata token",
            )?,
            missing_member: UnpairedMember::Bulk,
        });
    }
    for token in &container.rse.unpaired_bulk {
        projection.unpaired_segments.push(UnpairedSegmentRecord {
            id: retained_format(
                ctx,
                format_args!("inventor:rse:unpaired-bulk#{}", token.as_str()),
                "retain Inventor unpaired bulk id",
            )?,
            token: retained_clone(ctx, token.as_str(), "retain Inventor unpaired bulk token")?,
            missing_member: UnpairedMember::Metadata,
        });
    }
    Ok(projection)
}
