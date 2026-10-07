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
    let mut entries = ctx.collection_vec(entry_count, "nx summary entries")?;
    let semantic_streams = (scan.count(ctx, parasolid::StreamKind::Partition)? != 0)
        .then(|| native::substrate::topology_streams(ctx, scan))
        .transpose()?;

    for entry in ctx.admit_iter(&scan.container.entries, "NX summary directory traversal")? {
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
            name: ctx
                .format_retained(format_args!("{}", entry.name), "nx summary directory name")?,
            role: entry.content().role(),
            storage,
            attributes,
        });
    }

    let mut storage_notes_storage = ctx.reserve_scoped(0, "nx temporary storage notes")?;
    let mut storage_notes: Vec<String> = Vec::new();
    for (si, stream) in ctx
        .admit_iter(&scan.streams, "NX summary stream traversal")?
        .enumerate()
    {
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
            let graph = topology::Graph::parse(ctx, &stream.inflated)?;
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
                        graph.of_kind(kind).len(),
                    )),
                )?;
            }
            if stream.kind() == parasolid::StreamKind::Partition {
                let Some(semantic_streams) = semantic_streams.as_ref() else {
                    return Err(CodecError::malformed(
                        "NX partition has no topology byte views",
                    ));
                };
                let graph = topology::Graph::parse(ctx, &semantic_streams[si])?;
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
                            graph.of_kind(kind).len(),
                        )),
                    )?;
                }
            } else if stream.kind() == parasolid::StreamKind::Deltas {
                let census = deltas::census::walk(ctx, &stream.inflated)?;
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
                for (&family, &count) in
                    ctx.admit_iter(&census.full_counts(ctx)?, "NX summary full record counts")?
                {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.delta.full.",
                        family,
                        true,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                    )?;
                }
                for (&family, &count) in ctx.admit_iter(
                    &census.tombstone_counts(ctx)?,
                    "NX summary tombstone counts",
                )? {
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
        let inflated_len = cadmpeg_core::decode::u64_from_index(stream.inflated.len());
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
                        storage_notes_storage.with_storage(|| {
                            ctx.reserve_vec(&mut storage_notes, 1, "nx summary storage notes")
                        })?;
                        storage_notes.push(ctx.format_retained(
                            format_args!(
                                "parasolid#{si}: {message}: {}/{inflated_len}",
                                stream.consumed
                            ),
                            "nx summary storage note",
                        )?);
                        EntryStorage::payload_only(VerbatimLabel::Stored, inflated_len)
                    }
                }
            }
        };

        entries.push(ContainerEntry {
            name: ctx.format_retained(format_args!("parasolid#{si}"), "nx summary stream name")?,
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
    ctx.reserve_vec(
        &mut notes,
        storage_notes.len(),
        "nx combined inspection notes",
    )?;
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
        cadmpeg_core::decode::index_from_u32(number.ilog10() + 1)
    }
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

    let mut key = String::new();
    ctx.try_reserve_retained_text(&mut key, key_len, "nx summary attribute text")?;
    ctx.append_retained(&mut key, prefix, "NX admitted text append")?;
    if lowercase_suffix {
        for character in ctx.admit_iter(suffix, "NX summary attribute suffix traversal")? {
            ctx.push_retained_char(
                &mut key,
                character.to_ascii_lowercase(),
                "NX summary suffix character",
            )?;
        }
    } else {
        ctx.append_retained(&mut key, suffix, "NX admitted text append")?;
    }
    let mut rendered = String::new();
    ctx.try_reserve_retained_text(&mut rendered, value_len, "nx summary attribute text")?;
    match value {
        SummaryValue::Text(text) => {
            ctx.append_retained(&mut rendered, text, "NX admitted text append")?
        }
        SummaryValue::Number(number) => write!(&mut rendered, "{number}")
            .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?,
    }
    ctx.insert_btree_map(attributes, key, rendered, "nx summary attributes")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn summary_suffix_character_refusal_precedes_attribute_insertion() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        for (suffix, expected, character_bytes) in [("AB", "prefix:ab", 1), ("ÅB", "prefix:Åb", 2)]
        {
            let error = crate::test_support::resource_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                "NX summary suffix character",
                |ctx| {
                    let mut attributes = std::collections::BTreeMap::new();
                    let result = super::insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "prefix:",
                        suffix,
                        true,
                        super::SummaryValue::Text("text"),
                    );
                    if result.is_err() {
                        assert!(attributes.is_empty());
                    }
                    result
                },
            );
            // Character copying counts its encoded UTF-8 bytes.
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "NX summary suffix character"
                    && limit.additional == character_bytes));
            crate::test_support::with_decode_context(|ctx| {
                let mut attributes = std::collections::BTreeMap::new();
                super::insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "prefix:",
                    suffix,
                    true,
                    super::SummaryValue::Text("text"),
                )
                .unwrap();
                assert_eq!(attributes.get(expected).map(String::as_str), Some("text"));
            });
        }
    }

    #[test]
    fn inspection_partition_search_refuses_unadmitted_stream_work() {
        let file = crate::test_support::test_prt::single_part_prt();
        let container =
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
                .unwrap();
        let scan = crate::decode::Scan {
            container,
            streams: vec![crate::parasolid::Stream {
                file_offset: 0,
                consumed: 0,
                inflated: Vec::new(),
                body: crate::parasolid::StreamBody::Preview,
            }],
        };
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let error = super::summarize(ctx, &scan).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                        && limit.operation == "count NX streams")
                );
            },
        );
    }
}
