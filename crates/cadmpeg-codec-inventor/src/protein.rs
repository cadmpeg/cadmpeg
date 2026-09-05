// SPDX-License-Identifier: Apache-2.0
//! Inventor Protein package framing and inventory.

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::protein_header;

#[derive(Debug)]
pub(crate) enum ProteinState<'a> {
    Absent,
    Empty {
        stream: CompoundStreamId,
    },
    Package(ProteinEnvelope<'a>),
    Malformed {
        stream: CompoundStreamId,
        detail: String,
    },
}

#[derive(Debug)]
pub(crate) struct ProteinEnvelope<'a> {
    pub(crate) stream: CompoundStreamId,
    pub(crate) declared_len: u32,
    pub(crate) archive: ArchiveSnapshot<'a>,
    pub(crate) payload: View<'a>,
}

pub(crate) struct ProteinInstanceRecords {
    pub(crate) entry_name: String,
    pub(crate) records: Vec<cadmpeg_protein::DecodedRecord>,
    pub(crate) rejected: Vec<cadmpeg_protein::RejectedRecord>,
}

pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
) -> Result<ProteinState<'a>, CodecError> {
    let Some(stream) = snapshot.stream("Protein") else {
        return Ok(ProteinState::Absent);
    };
    let source = snapshot.open(ctx, stream)?;
    let result = parse_stream(ctx, source);
    Ok(match result {
        Ok(ParsedProtein::Empty) => ProteinState::Empty {
            stream: stream.id(),
        },
        Ok(ParsedProtein::Package {
            declared_len,
            archive,
            payload,
        }) => ProteinState::Package(ProteinEnvelope {
            stream: stream.id(),
            declared_len,
            archive,
            payload,
        }),
        Err(error) => ProteinState::Malformed {
            stream: stream.id(),
            detail: crate::issue_detail(error)?,
        },
    })
}

enum ParsedProtein<'a> {
    Empty,
    Package {
        declared_len: u32,
        archive: ArchiveSnapshot<'a>,
        payload: View<'a>,
    },
}

fn parse_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: cadmpeg_core::decode::View<'a>,
) -> Result<ParsedProtein<'a>, CodecError> {
    let mut header = source;
    let declared_len = header.req_u32_le()?;
    if declared_len == 0 {
        if source.window().len() != protein_header::LEN {
            return Err(CodecError::Malformed(
                "empty Inventor Protein stream has trailing bytes".into(),
            ));
        }
        return Ok(ParsedProtein::Empty);
    }
    let payload_len = source.window().len().saturating_sub(protein_header::LEN);
    if declared_len as usize != payload_len {
        return Err(CodecError::malformed(format_args!(
            "Inventor Protein declares {declared_len} bytes but stores {payload_len}"
        )));
    }
    let payload = source
        .child(source.start() + protein_header::LEN, source.end())
        .ok_or_else(|| CodecError::Malformed("Inventor Protein payload range is invalid".into()))?;
    let archive = ArchiveSnapshot::new(payload)?;
    for entry in archive.entries() {
        validate_entry_name(&entry.name)?;
    }
    ctx.charge_collection_items(
        archive.entries().len() as u64,
        "admit Inventor Protein package entries",
    )?;
    Ok(ParsedProtein::Package {
        declared_len,
        archive,
        payload,
    })
}

pub(crate) fn fuzz_parse_stream(ctx: &DecodeContext<'_>, source: View<'_>) {
    let _ = parse_stream(ctx, source);
}

pub(crate) fn decode_instances(
    ctx: &DecodeContext<'_>,
    package: &ProteinEnvelope<'_>,
) -> Result<Vec<ProteinInstanceRecords>, CodecError> {
    decode_instances_from(ctx, &package.archive, package.payload)
}

fn decode_instances_from(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    payload: View<'_>,
) -> Result<Vec<ProteinInstanceRecords>, CodecError> {
    if !cadmpeg_protein::has_schemas(payload.window()) {
        return Ok(Vec::new());
    }
    let entries = archive
        .entries()
        .iter()
        .filter(|entry| entry.name.ends_with("InstanceProperties.bin"))
        .collect::<Vec<_>>();
    ctx.charge_collection_items(
        entries.len() as u64,
        "admit Inventor Protein instance streams",
    )?;
    entries
        .into_iter()
        .map(|entry| {
            let instance = archive.open(ctx, &entry.name)?;
            let outcome = cadmpeg_protein::decode_detailed(payload.window(), instance.window())?;
            Ok(ProteinInstanceRecords {
                entry_name: entry.name.clone(),
                records: outcome.records,
                rejected: outcome.rejected,
            })
        })
        .collect()
}

fn validate_entry_name(name: &str) -> Result<(), CodecError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
        return Err(CodecError::malformed(format_args!(
            "Inventor Protein package has unsafe entry name {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_protein::{
        CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
    };
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn protein_distinguishes_empty_and_exact_package() {
        let empty = 0_u32.to_le_bytes();
        assert_eq!(empty.len(), 4);
        with_stream(&empty, |ctx, root| {
            assert_eq!(root.window().len(), 4);
            assert_eq!(
                u32::from_le_bytes(root.window().try_into().expect("empty payload length")),
                0
            );
            assert!(matches!(parse_stream(ctx, root), Ok(ParsedProtein::Empty)));
        });
        let zip = zip_fixture("Schemas/ExampleSchema.xml");
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().expect("planted payload length")),
            zip.len() as u32
        );
        with_stream(&bytes, |ctx, root| {
            let ParsedProtein::Package {
                declared_len,
                archive,
                ..
            } = parse_stream(ctx, root).expect("synthetic Protein package parses")
            else {
                panic!("package state")
            };
            assert_eq!(declared_len, zip.len() as u32);
            assert_eq!(archive.entries().len(), 1);
        });
    }

    #[test]
    fn protein_rejects_length_mismatch_and_unsafe_paths() {
        let mut mismatch = 5_u32.to_le_bytes().to_vec();
        mismatch.extend_from_slice(b"four");
        with_stream(&mismatch, |ctx, root| {
            assert!(parse_stream(ctx, root).is_err());
        });

        let zip = zip_fixture("../escape");
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        with_stream(&bytes, |ctx, root| {
            assert!(parse_stream(ctx, root).is_err());
        });
    }

    #[test]
    fn inventor_package_uses_shared_schema_instance_decoder() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        let mut record = Vec::new();
        for value in ["SimpleSchema", "asset-guid", "Simple", ""] {
            push_lp(&mut record, value);
        }
        push_lp(&mut record, &"x".repeat(160));
        let instance = paged_instance(&record);
        let zip = zip_entries(&[
            ("Schemas/SimpleSchema.xml", schema),
            ("AssetData/InstanceProperties.bin", &instance),
        ]);
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        with_stream(&bytes, |ctx, root| {
            let ParsedProtein::Package {
                declared_len,
                archive,
                payload,
            } = parse_stream(ctx, root).expect("synthetic Protein package parses")
            else {
                panic!("package state")
            };
            assert_eq!(declared_len as usize, payload.window().len());
            let instances =
                decode_instances_from(ctx, &archive, payload).expect("instances decode");
            assert_eq!(instances.len(), 1);
            assert_eq!(instances[0].records.len(), 1);
            assert_eq!(instances[0].records[0].schema, "SimpleSchema");
            assert_eq!(instances[0].records[0].guid, "asset-guid");
            assert!(instances[0].rejected.is_empty());
        });
    }

    #[test]
    fn inventor_package_retains_rejected_instance_positions() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        let mut valid = Vec::new();
        for value in ["SimpleSchema", "asset-guid", "Simple", ""] {
            push_lp(&mut valid, value);
        }
        push_lp(&mut valid, &"x".repeat(160));
        let mut instance = paged_instance(&valid);
        let malformed = paged_instance(&[0xff; 160]);
        instance.extend_from_slice(&malformed[16..]);
        let zip = zip_entries(&[
            ("Schemas/SimpleSchema.xml", schema),
            ("AssetData/InstanceProperties.bin", &instance),
        ]);
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        with_stream(&bytes, |ctx, root| {
            let ParsedProtein::Package {
                archive, payload, ..
            } = parse_stream(ctx, root).expect("synthetic Protein package parses")
            else {
                panic!("package state")
            };
            let instances =
                decode_instances_from(ctx, &archive, payload).expect("instances decode");
            assert_eq!(instances[0].records.len(), 1);
            assert_eq!(instances[0].rejected.len(), 1);
            assert_eq!(instances[0].rejected[0].ordinal, 1);
        });
    }

    fn with_stream(
        bytes: &[u8],
        test: impl FnOnce(&DecodeContext<'_>, cadmpeg_core::decode::View<'_>),
    ) {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic Protein stream fits policy");
        test(&ctx, root);
    }

    fn zip_fixture(name: &str) -> Vec<u8> {
        zip_entries(&[(name, b"synthetic")])
    }

    fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
                )
                .expect("start synthetic ZIP member");
            writer.write_all(bytes).expect("write synthetic ZIP member");
        }
        writer.finish().expect("finish synthetic ZIP").into_inner()
    }

    fn push_lp(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn paged_instance(record: &[u8]) -> Vec<u8> {
        const BODY_SIZE: usize = PAGE_SIZE - 8;
        let mut bytes = (PAGE_SIZE as u32).to_le_bytes().to_vec();
        bytes.resize(STREAM_HEADER_LEN, 0);
        let mut chunks = record.chunks(BODY_SIZE).peekable();
        let first = chunks.next().expect("record is nonempty");
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(RECORD_MARKER);
        bytes.extend_from_slice(first);
        bytes.resize(STREAM_HEADER_LEN + PAGE_SIZE, 0);
        while let Some(chunk) = chunks.next() {
            if chunks.peek().is_some() {
                bytes.extend_from_slice(&[0, 0, 0, 0]);
                bytes.extend_from_slice(CONTINUATION_MARKER);
            } else {
                bytes.extend_from_slice(TERMINAL_MARKER);
                bytes.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
            }
            bytes.extend_from_slice(chunk);
            let page_bytes = bytes.len() - STREAM_HEADER_LEN;
            let next_page = STREAM_HEADER_LEN + page_bytes.div_ceil(PAGE_SIZE) * PAGE_SIZE;
            bytes.resize(next_page, 0);
        }
        bytes
    }
}
