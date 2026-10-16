// SPDX-License-Identifier: Apache-2.0
//! NX inspection summary and its resource-admitted output records.

use std::collections::BTreeMap;

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::framing::node_kind::NodeKind;
use crate::{container, decode, deltas, native, parasolid, topology};

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
pub(super) fn summarize(scan: &decode::Scan) -> Result<ContainerSummary, CodecError> {
    let mut entries = Vec::new();
    let semantic_streams = (scan.count(parasolid::StreamKind::Partition) != 0)
        .then(|| native::substrate::topology_streams(scan))
        .transpose()?;

    for entry in &scan.container.entries {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            &mut attributes,
            "region",
            "",
            false,
            SummaryValue::Text(entry.region.label()),
        );
        let storage = match entry.file_span() {
            Some((off, size)) => {
                insert_summary_attribute(
                    &mut attributes,
                    "file_offset",
                    "",
                    false,
                    SummaryValue::Number(off),
                );
                EntryStorage::verbatim(VerbatimLabel::None, size)
            }
            None => {
                insert_summary_attribute(
                    &mut attributes,
                    "kind",
                    "",
                    false,
                    SummaryValue::Text("directory"),
                );
                EntryStorage::Directory
            }
        };
        entries.push(ContainerEntry {
            name: entry.name.clone(),
            role: entry.content().role(),
            storage,
            attributes,
        });
    }

    let mut storage_notes: Vec<String> = Vec::new();
    for (si, stream) in scan.streams.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            &mut attributes,
            "file_offset",
            "",
            false,
            SummaryValue::Number(cadmpeg_core::decode::u64_from_index(stream.file_offset)),
        );
        insert_summary_attribute(
            &mut attributes,
            "kind",
            "",
            false,
            SummaryValue::Text(stream.kind().label()),
        );
        if let Some(schema) = stream.schema_token() {
            insert_summary_attribute(
                &mut attributes,
                "schema",
                "",
                false,
                SummaryValue::Text(schema.value()),
            );
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
                    &mut attributes,
                    "records.",
                    name,
                    false,
                    SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                        graph.kind_count(kind),
                    )),
                );
            }
            if stream.kind() == parasolid::StreamKind::Partition {
                let Some(semantic_streams) = semantic_streams.as_ref() else {
                    return Err(CodecError::malformed(
                        "NX partition has no topology byte views",
                    ));
                };
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
                        &mut attributes,
                        "records.live.",
                        name,
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            graph.kind_count(kind),
                        )),
                    );
                }
            } else if stream.kind() == parasolid::StreamKind::Deltas {
                let census = deltas::census::walk(&stream.inflated)?;
                if census.transmit_header.is_some() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.transmit_headers",
                        "",
                        false,
                        SummaryValue::Text("1"),
                    );
                }
                if !census.body_revisions.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.body_revisions",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.body_revisions.len(),
                        )),
                    );
                }
                if !census.term_use_numeric_tails.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.term_use_numeric_tails",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.term_use_numeric_tails.len(),
                        )),
                    );
                }
                if !census.tagged_reference_lanes.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.tagged_reference_lanes",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.tagged_reference_lanes.len(),
                        )),
                    );
                }
                if !census.reference_type_maps.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.reference_type_maps",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.reference_type_maps.len(),
                        )),
                    );
                }
                if !census.reference_state_packets.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.reference_state_packets",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.reference_state_packets.len(),
                        )),
                    );
                }
                if !census.reference_marker_packets.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.reference_marker_packets",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.reference_marker_packets.len(),
                        )),
                    );
                }
                if !census.inline_schema_declarations.is_empty() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.inline_schema_declarations",
                        "",
                        false,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(
                            census.inline_schema_declarations.len(),
                        )),
                    );
                }
                for (family, count) in census.full_counts() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.full.",
                        family,
                        true,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                    );
                }
                for (family, count) in census.tombstone_counts() {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.delta.tombstone.",
                        family,
                        true,
                        SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                    );
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
                        storage_notes.push(format!(
                            "parasolid#{si}: {message}: {}/{inflated_len}",
                            stream.consumed
                        ));
                        EntryStorage::payload_only(VerbatimLabel::Stored, inflated_len)
                    }
                }
            }
        };

        entries.push(ContainerEntry {
            name: format!("parasolid#{si}"),
            role: if stream.kind().is_parasolid() {
                ContainerRole::ParasolidStream
            } else {
                ContainerRole::Preview
            },
            storage,
            attributes,
        });
    }

    let (classification, mut notes) = crate::scan_notes::summarize(scan)?;

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

fn insert_summary_attribute(
    attributes: &mut BTreeMap<String, String>,
    prefix: &str,
    suffix: &str,
    lowercase_suffix: bool,
    value: SummaryValue<'_>,
) {
    let mut key = prefix.to_owned();
    if lowercase_suffix {
        key.push_str(&suffix.to_ascii_lowercase());
    } else {
        key.push_str(suffix);
    }
    let rendered = match value {
        SummaryValue::Text(text) => text.to_owned(),
        SummaryValue::Number(number) => number.to_string(),
    };
    attributes.insert(key, rendered);
}
