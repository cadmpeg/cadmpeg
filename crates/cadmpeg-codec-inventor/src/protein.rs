// SPDX-License-Identifier: Apache-2.0
//! Inventor Protein package framing and inventory.

use std::num::NonZeroU32;

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::protein_header;
use crate::record_issue::admit_formatted;

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
    pub(crate) declared_len: NonZeroU32,
    pub(crate) archive: ArchiveSnapshot<'a>,
    payload: View<'a>,
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
        Err(error) => {
            if !matches!(error, CodecError::ResourceLimit(_)) {
                crate::record_issue::admit_issue_detail(
                    ctx,
                    &error,
                    "retain Inventor malformed Protein detail",
                )?;
            }
            ProteinState::Malformed {
                stream: stream.id(),
                detail: crate::issue_detail(error)?,
            }
        }
    })
}

enum ParsedProtein<'a> {
    Empty,
    Package {
        declared_len: NonZeroU32,
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
    let Some(declared_len) = NonZeroU32::new(declared_len) else {
        if source.window().len() != protein_header::LEN {
            return Err(CodecError::Malformed(
                "empty Inventor Protein stream has trailing bytes".into(),
            ));
        }
        return Ok(ParsedProtein::Empty);
    };
    let payload_len = source.window().len().saturating_sub(protein_header::LEN);
    if declared_len.get() as usize != payload_len {
        return Err(CodecError::malformed(format_args!(
            "Inventor Protein declares {declared_len} bytes but stores {payload_len}"
        )));
    }
    let payload = source
        .child(source.start() + protein_header::LEN, source.end())
        .ok_or_else(|| CodecError::Malformed("Inventor Protein payload range is invalid".into()))?;
    let archive = ArchiveSnapshot::new(ctx, payload)?;
    for entry in archive.entries() {
        validate_entry_name(ctx, &entry.name)?;
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
    let _probe = parse_stream(ctx, source);
}

pub(crate) fn decode_instances(
    ctx: &DecodeContext<'_>,
    package: &ProteinEnvelope<'_>,
) -> Result<Vec<ProteinInstanceRecords>, CodecError> {
    decode_instances_from(ctx, &package.archive, package.payload)
}

pub(crate) fn decode_instances_with_issue(
    ctx: &DecodeContext<'_>,
    package: &ProteinEnvelope<'_>,
) -> Result<(Vec<ProteinInstanceRecords>, Option<String>), CodecError> {
    match decode_instances(ctx, package) {
        Ok(instances) => Ok((instances, None)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            crate::record_issue::admit_issue_detail(
                ctx,
                &error,
                "retain Inventor Protein semantic issue",
            )?;
            Ok((Vec::new(), Some(crate::issue_detail(error)?)))
        }
    }
}

fn decode_instances_from(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    payload: View<'_>,
) -> Result<Vec<ProteinInstanceRecords>, CodecError> {
    let Some(mut catalog) = cadmpeg_protein::SchemaCatalog::load(ctx, payload)? else {
        return Ok(Vec::new());
    };
    let count = archive
        .entries()
        .iter()
        .filter(|entry| entry.name.ends_with("InstanceProperties.bin"))
        .count();
    ctx.charge_collection_items(count as u64, "admit Inventor Protein instance streams")?;
    let entries = archive
        .entries()
        .iter()
        .filter(|entry| entry.name.ends_with("InstanceProperties.bin"))
        .collect::<Vec<_>>();
    ctx.charge_collection_items(count as u64, "admit Inventor Protein instance records")?;
    entries
        .into_iter()
        .map(|entry| {
            let instance = archive.open(ctx, &entry.name)?;
            let frames = cadmpeg_protein::framing::record_frames_admitted(ctx, instance.window())?;
            let outcome = cadmpeg_protein::decode_frames_admitted(ctx, &mut catalog, &frames)?;
            ctx.charge_retained(
                entry.name.len() as u64,
                "Inventor Protein instance entry name",
            )?;
            Ok(ProteinInstanceRecords {
                entry_name: entry.name.clone(),
                records: outcome.records,
                rejected: outcome.rejected,
            })
        })
        .collect()
}

fn validate_entry_name(ctx: &DecodeContext<'_>, name: &str) -> Result<(), CodecError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
        admit_formatted(
            ctx,
            format_args!("Inventor Protein package has unsafe entry name {name:?}"),
            "retain Inventor unsafe Protein entry diagnostic",
        )?;
        return Err(CodecError::malformed(format_args!(
            "Inventor Protein package has unsafe entry name {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use cadmpeg_protein::{
        CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
    };
    use zip::write::SimpleFileOptions;

    use super::{
        decode_instances_from, decode_instances_with_issue, parse_stream, validate_entry_name,
        ParsedProtein, ProteinEnvelope,
    };
    use cadmpeg_core::decode::DecodeContext;

    #[test]
    fn unsafe_protein_entry_diagnostic_refuses_retained_limit_before_format() {
        let name = "../escape";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
        assert!(matches!(
            validate_entry_name(&limited, name),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
        ));
        let (service, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        let error = validate_entry_name(&service, name).expect_err("unsafe path rejected");
        assert!(error.to_string().contains("../escape"));
    }

    #[test]
    fn malformed_protein_detail_refuses_retained_limit_before_copy() {
        let mut bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&[0; 4]);
        let entry_start = 512 + 3 * 128;
        let name = "Protein";
        bytes[entry_start..entry_start + 64].fill(0);
        for (index, unit) in name.encode_utf16().enumerate() {
            let offset = entry_start + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let name_len = ((name.encode_utf16().count() + 1) * 2) as u16;
        bytes[entry_start + 64..entry_start + 66].copy_from_slice(&name_len.to_le_bytes());
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("compound input fits service policy");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("synthetic compound parses");
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            super::parse(&limited, &snapshot),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor malformed Protein detail"
        ));
        assert!(matches!(
            super::parse(&setup, &snapshot).expect("malformed Protein remains an inventory state"),
            super::ProteinState::Malformed { .. }
        ));
    }

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
            assert_eq!(declared_len.get(), zip.len() as u32);
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
            assert_eq!(declared_len.get() as usize, payload.window().len());
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
    fn protein_instance_result_vec_refuses_collection_limit_before_collect() {
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
                archive, payload, ..
            } = parse_stream(ctx, root).expect("synthetic Protein package parses")
            else {
                panic!("package state")
            };
            assert_eq!(
                decode_instances_from(ctx, &archive, payload)
                    .expect("service profile decodes instance")
                    .len(),
                1
            );
            let mut result_vec_refused = false;
            for cap in 0..64 {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (limited, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("synthetic Protein input fits policy");
                if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
                    decode_instances_from(&limited, &archive, payload)
                {
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "admit Inventor Protein instance records"
                    {
                        result_vec_refused = true;
                        break;
                    }
                }
            }
            assert!(
                result_vec_refused,
                "result collection must refuse at its own admission"
            );
        });
    }

    #[test]
    fn protein_semantic_issue_refuses_retained_limit_before_copy() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        let zip = zip_entries(&[
            ("Schemas/SimpleSchema.xml", schema),
            ("AssetData/InstanceProperties.bin", b"bad"),
        ]);
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("package context");
        let ParsedProtein::Package {
            declared_len,
            archive,
            payload,
        } = parse_stream(&setup, root).expect("ZIP package parses")
        else {
            panic!("package state");
        };
        let cfb = crate::test_support::test_fixtures::fixture(true);
        let (cfb_ctx, cfb_root) =
            DecodeContext::from_root_bytes(&cfb, &arena, &DecodePolicy::service())
                .expect("compound context");
        let snapshot = CompoundSnapshot::new(&cfb_ctx, cfb_root).expect("compound fixture");
        let stream = snapshot
            .stream("RSeStorage/RSeSegInfo")
            .expect("fixture stream")
            .id();
        let package = ProteinEnvelope {
            stream,
            declared_len,
            archive,
            payload,
        };
        let (_, issue) = decode_instances_with_issue(&setup, &package)
            .expect("service policy retains semantic issue");
        assert!(issue.is_some());

        let mut cap = 0;
        let mut needed = None;
        for _ in 0..128 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (limited, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
            match decode_instances_with_issue(&limited, &package) {
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes =>
                {
                    let next = limit
                        .used
                        .checked_add(limit.additional)
                        .expect("fixture charge fits u64");
                    if limit.operation == "retain Inventor Protein semantic issue" {
                        needed = Some(next);
                        break;
                    }
                    assert!(next > cap, "fixture advances to its next retained charge");
                    cap = next;
                }
                Ok(_) => panic!("expected Protein retained refusal"),
                Err(error) => panic!("expected Protein retained refusal: {error:?}"),
            }
        }
        let needed = needed.expect("semantic issue reached within fixture charges");
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = needed - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            decode_instances_with_issue(&limited, &package),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor Protein semantic issue"
                    && limit.limit == needed - 1
        ));
    }

    #[test]
    fn inventor_reuses_one_charged_schema_catalog_across_instance_streams() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        let mut record = Vec::new();
        for value in ["SimpleSchema", "asset-guid", "Simple", ""] {
            push_lp(&mut record, value);
        }
        push_lp(&mut record, &"x".repeat(160));
        let instance = paged_instance(&record);
        let zip = zip_entries(&[
            ("Schemas/SimpleSchema.xml", schema),
            ("First/InstanceProperties.bin", &instance),
            ("Second/InstanceProperties.bin", &instance),
        ]);
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two three-entry ZIP inventories, one schema parse, and one closure.
        policy.limits.max_work_units = schema.len() as u64 + 10;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("package fits the service input limit");
        let ParsedProtein::Package {
            archive, payload, ..
        } = parse_stream(&ctx, root).expect("package parses")
        else {
            panic!("package state")
        };
        let instances = decode_instances_from(&ctx, &archive, payload)
            .expect("one schema parse admits both streams");
        assert_eq!(instances.len(), 2);
        assert!(instances.iter().all(|instance| instance.records.len() == 1));
        assert!(matches!(
            ctx.charge_work(1, "prove schema parse was charged"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
        ));
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
