// SPDX-License-Identifier: Apache-2.0
//! NX inspection summary and its resource-admitted output records.

use std::collections::BTreeMap;

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::framing::node_kind::NodeKind;
use crate::{container, decode, parasolid, topology};

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
pub(super) fn summarize(scan: &decode::Scan) -> Result<ContainerSummary, CodecError> {
    let mut entries = Vec::new();

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
        if stream.kind().is_parasolid() && stream.kind() != parasolid::StreamKind::Deltas {
            let graph = topology::Graph::parse_source(&stream.inflated)?;
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
                let count = cadmpeg_core::decode::u64_from_index(graph.kind_count(kind));
                insert_summary_attribute(
                    &mut attributes,
                    "records.",
                    name,
                    false,
                    SummaryValue::Number(count),
                );
                if stream.kind() == parasolid::StreamKind::Partition {
                    insert_summary_attribute(
                        &mut attributes,
                        "records.live.",
                        name,
                        false,
                        SummaryValue::Number(count),
                    );
                }
            }
        } else if stream.kind() == parasolid::StreamKind::Deltas {
            let owner = crate::records::DeltasRecords::read(&stream.inflated)?;
            let census = owner.census();
            for (key, count) in [
                ("records.delta.instances", census.instance_count()),
                ("records.delta.schemas", census.schema_count()),
                ("records.delta.bytes_decoded", census.bytes_decoded()),
            ] {
                insert_summary_attribute(
                    &mut attributes,
                    key,
                    "",
                    false,
                    SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                );
            }
            insert_summary_attribute(
                &mut attributes,
                "records.delta.selection",
                "",
                false,
                SummaryValue::Text("unresolved"),
            );
            for (family, count) in census.full_counts() {
                insert_summary_attribute(
                    &mut attributes,
                    "records.delta.full.",
                    family,
                    true,
                    SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                );
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

#[cfg(test)]
mod tests {
    #[test]
    fn deltas_inspection_counts_only_owned_instances() {
        let file = crate::test_support::test_prt::single_part_prt();
        crate::test_support::with_decode_context(|_ctx| {
            let scan = crate::decode::Scan {
                container: crate::container::scan_bytes(file).unwrap(),
                streams: vec![crate::test_support::owned_deltas::stream(
                    crate::test_support::test_pmarks::complete(
                        crate::test_support::owned_deltas::body_stream(36),
                    ),
                )],
            };
            let summary = super::summarize(&scan).unwrap();
            let attributes = &summary.entries.last().unwrap().attributes;
            assert_eq!(
                attributes
                    .get("records.delta.instances")
                    .map(String::as_str),
                Some("1")
            );
            assert_eq!(
                attributes
                    .get("records.delta.full.body")
                    .map(String::as_str),
                Some("1")
            );
            assert!(!attributes.contains_key("records.body"));
            assert!(!attributes.contains_key("records.live.body"));
        });
    }
}
