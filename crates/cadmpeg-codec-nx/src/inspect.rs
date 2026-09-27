// SPDX-License-Identifier: Apache-2.0
//! NX inspection summary and its resource-admitted output records.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::framing::node_kind::NodeKind;
use crate::{container, decode, deltas, native, parasolid, topology};

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
pub(super) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &decode::Scan,
) -> Result<ContainerSummary, CodecError> {
    let entry_count = scan
        .container
        .entries
        .len()
        .checked_add(scan.streams.len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary entries", 0, u64::MAX))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(entry_count),
        "nx summary entries",
    )?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entry_count)
        .map_err(|_| ctx.refuse_codec_limit("nx summary entries", 0, 1))?;
    let semantic_streams = native::substrate::topology_streams(ctx, scan)?;

    for entry in &scan.container.entries {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "region",
            "",
            false,
            SummaryValue::Text(entry.region.label()),
        )?;
        let storage = match entry.file_span() {
            Some((off, size)) => {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "file_offset",
                    "",
                    false,
                    SummaryValue::Number(off),
                )?;
                EntryStorage::verbatim(VerbatimLabel::None, size)
            }
            None => {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "kind",
                    "",
                    false,
                    SummaryValue::Text("directory"),
                )?;
                EntryStorage::Directory
            }
        };
        entries.push(ContainerEntry {
            name: render_summary_text(
                ctx,
                "nx summary directory name",
                entry.name.len(),
                format_args!("{}", entry.name),
            )?,
            role: entry.content().role(),
            storage,
            attributes,
        });
    }

    let mut storage_notes: Vec<String> = Vec::new();
    for (si, stream) in scan.streams.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "file_offset",
            "",
            false,
            SummaryValue::Number(cadmpeg_core::decode::u64_from_index(stream.file_offset)),
        )?;
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "kind",
            "",
            false,
            SummaryValue::Text(stream.kind().label()),
        )?;
        if let Some(schema) = stream.schema_token() {
            insert_summary_attribute(
                ctx,
                &mut attributes,
                "schema",
                "",
                false,
                SummaryValue::Text(schema.value()),
            )?;
        }
        if stream.kind().is_parasolid() {
            let graph = topology::Graph::parse(&stream.inflated);
            for (kind, name) in [
                (NodeKind::Body, "body"),
                (NodeKind::Shell, "shell"),
                (NodeKind::Face, "face"),
                (NodeKind::Loop, "loop"),
                (NodeKind::Edge, "edge"),
                (NodeKind::Fin, "fin"),
                (NodeKind::Vertex, "vertex"),
                (NodeKind::Region, "region"),
            ] {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "records.",
                    name,
                    false,
                    SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                        graph.of_kind(kind).count(),
                    )),
                )?;
            }
            if stream.kind() == parasolid::StreamKind::Partition {
                let graph = topology::Graph::parse(&semantic_streams[si]);
                for (kind, name) in [
                    (NodeKind::Body, "body"),
                    (NodeKind::Shell, "shell"),
                    (NodeKind::Face, "face"),
                    (NodeKind::Loop, "loop"),
                    (NodeKind::Edge, "edge"),
                    (NodeKind::Fin, "fin"),
                    (NodeKind::Vertex, "vertex"),
                    (NodeKind::Region, "region"),
                ] {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.live.",
                        name,
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            graph.of_kind(kind).count(),
                        )),
                    )?;
                }
            } else if stream.kind() == parasolid::StreamKind::Deltas {
                let census = deltas::census::walk(&stream.inflated);
                if census.transmit_header.is_some() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.transmit_headers",
                        "",
                        false,
                        SummaryValue::Text("1"),
                    )?;
                }
                if !census.body_revisions.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.body_revisions",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.body_revisions.len(),
                        )),
                    )?;
                }
                if !census.term_use_numeric_tails.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.term_use_numeric_tails",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.term_use_numeric_tails.len(),
                        )),
                    )?;
                }
                if !census.tagged_reference_lanes.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.tagged_reference_lanes",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.tagged_reference_lanes.len(),
                        )),
                    )?;
                }
                if !census.reference_type_maps.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.reference_type_maps",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.reference_type_maps.len(),
                        )),
                    )?;
                }
                if !census.reference_state_packets.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.reference_state_packets",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.reference_state_packets.len(),
                        )),
                    )?;
                }
                if !census.reference_marker_packets.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.reference_marker_packets",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.reference_marker_packets.len(),
                        )),
                    )?;
                }
                if !census.inline_schema_declarations.is_empty() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.inline_schema_declarations",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.inline_schema_declarations.len(),
                        )),
                    )?;
                }
                for (family, count) in census.full_counts() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.full.",
                        family,
                        true,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                    )?;
                }
                for (family, count) in census.tombstone_counts() {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.tombstone.",
                        family,
                        true,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                    )?;
                }
            }
        }
        let inflated_len = stream.inflated.len() as u64;
        let storage = match scan.container.layout {
            container::ContainerLayout::Modern { .. } => EntryStorage::Compressed {
                method: CompressionMethod::Zlib,
                stored: None,
                expanded: Some(inflated_len),
            },
            container::ContainerLayout::LegacyCfb { .. } => {
                match EntryStorage::framed(VerbatimLabel::Stored, inflated_len, stream.consumed) {
                    Ok(storage) => storage,
                    Err(message) => {
                        let note_len = [
                            "parasolid#".len(),
                            decimal_len(cadmpeg_core::decode::u64_from_index(si)),
                            ": ".len(),
                            message.len(),
                            ": ".len(),
                            decimal_len(stream.consumed),
                            "/".len(),
                            decimal_len(inflated_len),
                        ]
                        .into_iter()
                        .try_fold(0usize, usize::checked_add)
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit("nx summary storage note", 0, u64::MAX)
                        })?;
                        ctx.charge_collection_items(1, "nx summary storage notes")?;
                        storage_notes.try_reserve(1).map_err(|_| {
                            ctx.refuse_codec_limit("nx summary storage notes", 0, 1)
                        })?;
                        storage_notes.push(render_summary_text(
                            ctx,
                            "nx summary storage note",
                            note_len,
                            format_args!(
                                "parasolid#{si}: {message}: {}/{inflated_len}",
                                stream.consumed
                            ),
                        )?);
                        EntryStorage::payload_only(VerbatimLabel::Stored, inflated_len)
                    }
                }
            }
        };
        let name_len = "parasolid#"
            .len()
            .checked_add(decimal_len(cadmpeg_core::decode::u64_from_index(si)))
            .ok_or_else(|| ctx.refuse_codec_limit("nx summary stream name", 0, u64::MAX))?;
        entries.push(ContainerEntry {
            name: render_summary_text(
                ctx,
                "nx summary stream name",
                name_len,
                format_args!("parasolid#{si}"),
            )?,
            role: if stream.kind().is_parasolid() {
                ContainerRole::ParasolidStream
            } else {
                ContainerRole::Preview
            },
            storage,
            attributes,
        });
    }

    let (classification, mut notes) = crate::scan_notes::summarize(ctx, scan)?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(storage_notes.len()),
        "nx combined inspection notes",
    )?;
    notes
        .try_reserve(storage_notes.len())
        .map_err(|_| ctx.refuse_codec_limit("nx combined inspection notes", 0, 1))?;
    notes.extend(storage_notes);
    let container_kind = classification.container_kind();
    let (dialects, dialect_losses) = classification.into_report_parts();
    Ok(ContainerSummary::classified(
        dialects,
        container_kind,
        entries,
        dialect_losses,
        notes,
    ))
}

#[derive(Clone, Copy)]
enum SummaryValue<'a> {
    Text(&'a str),
    Number(u64),
}

fn decimal_len(number: u64) -> usize {
    if number == 0 {
        1
    } else {
        usize::try_from(u64::from(number.ilog10()) + 1).unwrap_or(usize::MAX)
    }
}

fn render_summary_text(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    len: usize,
    args: std::fmt::Arguments<'_>,
) -> Result<String, CodecError> {
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(len), operation)?;
    let mut text = String::new();
    text.try_reserve_exact(len)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    text.write_fmt(args)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(text)
}

fn insert_summary_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    prefix: &str,
    suffix: &str,
    lowercase_suffix: bool,
    value: SummaryValue<'_>,
) -> Result<(), CodecError> {
    let key_len = prefix
        .len()
        .checked_add(suffix.len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary attribute text", 0, u64::MAX))?;
    let value_len = match value {
        SummaryValue::Text(text) => text.len(),
        SummaryValue::Number(number) => decimal_len(number),
    };
    let text_len = key_len
        .checked_add(value_len)
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary attribute text", 0, u64::MAX))?;
    ctx.charge_collection_items(1, "nx summary attributes")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(text_len),
        "nx summary attribute text",
    )?;
    let mut key = String::new();
    key.try_reserve_exact(key_len)
        .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?;
    key.push_str(prefix);
    if lowercase_suffix {
        for character in suffix.chars() {
            key.push(character.to_ascii_lowercase());
        }
    } else {
        key.push_str(suffix);
    }
    let mut rendered = String::new();
    rendered
        .try_reserve_exact(value_len)
        .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?;
    match value {
        SummaryValue::Text(text) => rendered.push_str(text),
        SummaryValue::Number(number) => write!(&mut rendered, "{number}")
            .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?,
    }
    attributes.insert(key, rendered);
    Ok(())
}
