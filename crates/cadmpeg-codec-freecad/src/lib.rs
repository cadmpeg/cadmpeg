// SPDX-License-Identifier: Apache-2.0
//! Read ZIP-packaged `FreeCAD` `.FCStd` documents.

mod container;
mod native;
mod persistence;

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;

/// Input-only `FCStd` codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct FcstdCodec;

impl Codec for FcstdCodec {
    fn id(&self) -> &'static str {
        "fcstd"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(b"PK\x03\x04") {
            return Confidence::No;
        }
        if contains(prefix, b"Document.xml")
            && contains(prefix, b"SchemaVersion")
            && contains(prefix, b"FileVersion")
        {
            Confidence::High
        } else if contains(prefix, b"Document.xml") {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        container::scan(reader).map(|scan| container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        let scan = container::scan(reader)?;
        if !options.container_only
            && (scan.document.schema_version != "4" || scan.document.file_version != "1")
        {
            return Err(CodecError::NotImplemented(format!(
                "FCStd SchemaVersion={} FileVersion={} persistence layout",
                scan.document.schema_version, scan.document.file_version
            )));
        }
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "schema_version".into(),
            scan.document.schema_version.clone(),
        );
        attributes.insert("file_version".into(), scan.document.file_version.clone());
        attributes.insert("document_root".into(), scan.document.root_name.clone());
        attributes.insert(
            "object_count".into(),
            scan.document.object_count.to_string(),
        );
        attributes.insert("archive_entry_count".into(), scan.entries.len().to_string());
        attributes.insert(
            "physical_ledger_spans".into(),
            scan.ledger.len().to_string(),
        );
        if let Some(value) = &scan.document.program_version {
            attributes.insert("program_version".into(), value.clone());
        }
        let thumbnail = scan
            .data
            .get("thumbnails/Thumbnail.png")
            .map(|bytes| ("thumbnails/Thumbnail.png", bytes))
            .or_else(|| {
                scan.data
                    .get("Thumbnail.png")
                    .map(|bytes| ("Thumbnail.png", bytes))
            });
        if let Some((_, thumbnail)) = thumbnail {
            attributes.insert("thumbnail_bytes".into(), thumbnail.len().to_string());
        }
        let mut ir = CadIr::empty(Units::default());
        ir.source = Some(SourceMeta {
            format: "fcstd".into(),
            attributes,
        });
        if let Some((name, bytes)) = thumbnail {
            ir.set_native_unknowns(
                "fcstd",
                &[UnknownRecord {
                    id: UnknownId(format!("fcstd:entry:{name}")),
                    offset: 0,
                    byte_len: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                    data: Some(bytes.clone()),
                    links: vec!["fcstd:document#0".into()],
                }],
            )?;
        }
        let namespace = ir.native.namespace_mut("fcstd");
        namespace.version = native::VERSION;
        namespace.set_arena("document", std::slice::from_ref(&scan.document))?;
        namespace.set_arena("physical_ledger", &scan.ledger)?;
        if !options.container_only {
            let document_bytes = scan.data.get("Document.xml").ok_or_else(|| {
                CodecError::Malformed("Document.xml disappeared after scan".into())
            })?;
            let graph = persistence::parse(document_bytes)?;
            for property in &graph.properties {
                for side_entry in &property.side_entries {
                    if !scan.data.contains_key(side_entry) {
                        return Err(CodecError::Malformed(format!(
                            "property {} references missing side entry {side_entry}",
                            property.id
                        )));
                    }
                }
            }
            let entry_records = scan
                .entries
                .iter()
                .map(|entry| {
                    let bytes = scan.data.get(&entry.name).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "entry {} disappeared after scan",
                            entry.name
                        ))
                    })?;
                    let referenced_by = graph
                        .properties
                        .iter()
                        .filter(|property| property.side_entries.contains(&entry.name))
                        .map(|property| property.id.clone())
                        .collect();
                    Ok(native::EntryRecord {
                        id: format!("fcstd:entry:{}", entry.name),
                        name: entry.name.clone(),
                        role: entry.role.clone(),
                        byte_len: bytes.len() as u64,
                        sha256: sha256_hex(bytes),
                        referenced_by,
                        data: bytes.clone(),
                    })
                })
                .collect::<Result<Vec<_>, CodecError>>()?;
            let logical_ledger = logical_ledger(&entry_records, &graph.properties)?;
            namespace.set_arena("objects", &graph.objects)?;
            namespace.set_arena("properties", &graph.properties)?;
            namespace.set_arena("entries", &entry_records)?;
            namespace.set_arena("logical_ledger", &logical_ledger)?;
        }
        let losses = if options.container_only {
            Vec::new()
        } else {
            vec![LossNote {
                category: LossCategory::Geometry,
                severity: Severity::Blocking,
                message: "FCStd persistence and exact-shape decoding are not implemented yet"
                    .into(),
                provenance: None,
            }]
        };
        Ok(DecodeResult::new(
            ir,
            DecodeReport {
                format: "fcstd".into(),
                container_only: options.container_only,
                geometry_transferred: false,
                losses,
                notes: container::summarize(&scan).notes,
            },
        ))
    }
}

fn logical_ledger(
    entries: &[native::EntryRecord],
    properties: &[native::PropertyRecord],
) -> Result<Vec<native::LogicalSpan>, CodecError> {
    let mut output = Vec::new();
    for entry in entries {
        if entry.name == "Document.xml" {
            let mut ranges = properties
                .iter()
                .map(|property| {
                    (
                        property.byte_start,
                        property.byte_end,
                        if property.family == native::PropertyFamily::Unknown {
                            "named_opaque"
                        } else {
                            "typed"
                        },
                        property.id.clone(),
                    )
                })
                .collect::<Vec<_>>();
            ranges.sort_by_key(|range| range.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len {
                    return Err(CodecError::Malformed(
                        "overlapping or invalid Document.xml property spans".into(),
                    ));
                }
                push_logical_span(&mut output, entry, cursor, start, "structural", None);
                push_logical_span(&mut output, entry, start, end, classification, Some(owner));
                cursor = end;
            }
            push_logical_span(
                &mut output,
                entry,
                cursor,
                entry.byte_len,
                "structural",
                None,
            );
        } else {
            let owner = entry
                .referenced_by
                .first()
                .cloned()
                .unwrap_or_else(|| entry.id.clone());
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len,
                "named_opaque",
                Some(owner),
            );
        }
    }
    Ok(output)
}

fn push_logical_span(
    output: &mut Vec<native::LogicalSpan>,
    entry: &native::EntryRecord,
    start: u64,
    end: u64,
    classification: &str,
    owner: Option<String>,
) {
    if start < end {
        output.push(native::LogicalSpan {
            id: format!("fcstd:logical-span#{}", output.len()),
            entry: entry.name.clone(),
            start,
            end,
            classification: classification.into(),
            owner,
        });
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests;
