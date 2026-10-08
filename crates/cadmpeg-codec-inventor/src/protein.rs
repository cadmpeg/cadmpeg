// SPDX-License-Identifier: Apache-2.0
//! Inventor Protein package framing and inventory.

use std::num::NonZeroU32;

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
    let Some(stream) = snapshot.stream(ctx, "Protein")? else {
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
            detail: crate::issue_detail(ctx, error, "retain Inventor malformed Protein detail")?,
        },
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
    let payload_len = source
        .window()
        .len()
        .checked_sub(protein_header::LEN)
        .ok_or_else(|| CodecError::Malformed("Inventor Protein header is truncated".into()))?;
    if usize::try_from(declared_len.get())
        .map_err(|_| CodecError::Malformed("Inventor numeric value exceeds target range".into()))?
        != payload_len
    {
        return Err(CodecError::malformed(format_args!(
            "Inventor Protein declares {declared_len} bytes but stores {payload_len}"
        )));
    }
    let payload = source
        .child(source.start() + protein_header::LEN, source.end())
        .ok_or_else(|| CodecError::Malformed("Inventor Protein payload range is invalid".into()))?;
    let archive = ArchiveSnapshot::new(ctx, payload)?;
    let mut entries = archive.entries().iter();
    while let Some(entry) =
        ctx.next_charged(&mut entries, "validate Inventor Protein entry names")?
    {
        validate_entry_name(ctx, &entry.name)?;
    }
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
        Err(error) => Ok((
            Vec::new(),
            Some(crate::issue_detail(
                ctx,
                error,
                "retain Inventor Protein semantic issue",
            )?),
        )),
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
    let mut instances = Vec::new();
    let mut entries = archive.entries().iter();
    while let Some(entry) =
        ctx.next_charged(&mut entries, "collect Inventor Protein instance streams")?
    {
        if !entry.name.ends_with("InstanceProperties.bin") {
            continue;
        }
        let instance = archive.open(ctx, &entry.name)?;
        let frames = cadmpeg_protein::framing::record_frames_admitted(ctx, instance.window())?;
        let outcome =
            cadmpeg_protein::decode_frames_admitted(ctx, &mut catalog, frames.frames())?;
        ctx.push_vec(
            &mut instances,
            ProteinInstanceRecords {
                entry_name: ctx
                    .copy_retained_text(&entry.name, "Inventor Protein instance entry name")?,
                records: outcome.records,
                rejected: outcome.rejected,
            },
            "admit Inventor Protein instance records",
        )?;
    }
    Ok(instances)
}

fn validate_entry_name(ctx: &DecodeContext<'_>, name: &str) -> Result<(), CodecError> {
    // One pass rejects a backslash or NUL byte and any empty, `.` or `..`
    // component: `component` is 1 or 2 after a leading `.` or `..`, 3 once
    // the component holds anything else.
    if name.is_empty() || name.starts_with('/') || {
        let mut component = 0_u8;
        let has_unsafe_byte = ctx.any_by(
            name.as_bytes(),
            |byte| {
                Ok(match *byte {
                    b'\\' | 0 => true,
                    b'/' => {
                        let invalid = component != 3;
                        component = 0;
                        invalid
                    }
                    byte => {
                        component = match component {
                            0 if byte == b'.' => 1,
                            1 if byte == b'.' => 2,
                            _ => 3,
                        };
                        false
                    }
                })
            },
            "validate Inventor Protein entry name",
        )?;
        has_unsafe_byte || component != 3
    } {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("Inventor Protein package has unsafe entry name {name:?}"),
            "retain Inventor unsafe Protein entry diagnostic",
        )?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
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
    fn protein_package_validation_does_not_add_archive_slots() {
        let zip = zip_fixture("Schemas/ExampleSchema.xml");
        let mut bytes = u32::try_from(zip.len())
            .expect("fixture length")
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(&zip);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One entry uses five slots: dependency index, raw-name set,
        // entry vector, decoded-name set and name index. Validation stores none.
        policy.limits.max_collection_items = 5;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let ParsedProtein::Package { archive, .. } =
            parse_stream(&ctx, root).expect("five archive slots")
        else {
            panic!("package state");
        };
        assert_eq!(archive.entries().len(), 1);
        assert!(matches!(
            ctx.charge_collection_items(1, "probe archive slots"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == 5 && limit.additional == 1
        ));
    }

    #[test]
    fn protein_name_validation_admits_only_the_next_archive_entry() {
        let entries = [
            ("Schemas/First.xml", &b"first"[..]),
            ("Schemas/Second.xml", &b"second"[..]),
        ];
        for count in [1, 2] {
            let zip = zip_entries(&entries[..count]);
            let mut bytes = u32::try_from(zip.len())
                .expect("fixture length")
                .to_le_bytes()
                .to_vec();
            bytes.extend_from_slice(&zip);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
            let probe = RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "validate Inventor Protein entry names",
                Some(1),
            );
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = parse_stream(&ctx, root)
            else {
                drop(probe);
                panic!("the first archive-name step must be charged before validation");
            };
            drop(probe);
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "validate Inventor Protein entry names");
            assert_eq!(limit.additional, 1);
            assert!(matches!(
                ctx.finish_session(),
                Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit
            ));
        }
    }

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
        let name_len =
            u16::try_from((name.encode_utf16().count() + 1) * 2).expect("fixture value fits u16");
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
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(&zip);
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().expect("planted payload length")),
            u32::try_from(zip.len()).expect("fixture value fits u32")
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
            assert_eq!(
                declared_len.get(),
                u32::try_from(zip.len()).expect("fixture value fits u32")
            );
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
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
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
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
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
            assert_eq!(
                cadmpeg_core::decode::index_from_u32(declared_len.get()),
                payload.window().len()
            );
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
    fn protein_instance_scan_does_not_prepay_the_tail_before_a_frame_error() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        for tail_count in [0_usize, 512] {
            let names: Vec<_> = (0..tail_count)
                .map(|index| format!("Tail/{index}/InstanceProperties.bin"))
                .collect();
            let mut entries = vec![
                ("Schemas/SimpleSchema.xml", &schema[..]),
                ("AssetData/InstanceProperties.bin", &b"bad"[..]),
            ];
            entries.extend(names.iter().map(|name| (name.as_str(), &b"bad"[..])));
            let zip = zip_entries(&entries);
            let mut bytes = u32::try_from(zip.len()).expect("fixture length")
                .to_le_bytes().to_vec();
            bytes.extend_from_slice(&zip);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("Protein scan context");
            let ParsedProtein::Package { archive, payload, .. } =
                parse_stream(&ctx, root).expect("Protein scan package")
            else {
                panic!("package state");
            };
            let probe = RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "collect Inventor Protein instance streams",
                Some(cadmpeg_core::decode::u64_from_index(archive.entries().len())),
            );
            assert!(matches!(decode_instances_from(&ctx, &archive, payload),
                Err(cadmpeg_core::CodecError::Malformed(detail))
                    if detail == "Protein page stream is shorter than its header and one page"));
            drop(probe);
ctx.finish_session().expect("instance-stream pass stops at the first malformed frame");
        }
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
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(&zip);
        with_stream(&bytes, |ctx, root| {
            const RESULT_COLLECTION_PRIOR_ITEMS: u64 = 293;
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
            // Original prior slots: ZIP index10 + schema view1 + XML tree/depth58
            // + schema maps2 + entry/view2 + frames213 + outcome1 + inheritance,
            // active-set/path/closure4 + resolved schema1 + property1 =293.
            // Removing the selected-entry Vec removes one actual slot. The
            // original cap now fits the final result; one less refuses its push.
            for cap in [RESULT_COLLECTION_PRIOR_ITEMS, RESULT_COLLECTION_PRIOR_ITEMS - 1] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (limited, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("synthetic Protein input fits policy");
                let result = decode_instances_from(&limited, &archive, payload);
                if cap == RESULT_COLLECTION_PRIOR_ITEMS {
                    assert_eq!(result.expect("original cap fits after removing the entry slot").len(), 1);
                    limited.finish_session().expect("only actual result storage is charged");
                } else {
                    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                        panic!("result collection must refuse before its own push");
                    };
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    assert_eq!(limit.operation, "admit Inventor Protein instance records");
                    assert_eq!(limit.used, RESULT_COLLECTION_PRIOR_ITEMS - 1);
                    assert_eq!(limit.additional, 1);
                    assert!(matches!(limited.finish_session(),
                        Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit));
                }
            }
        });
    }

    #[test]
    fn protein_semantic_issue_refuses_retained_limit_before_copy() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        let zip = zip_entries(&[
            ("Schemas/SimpleSchema.xml", schema),
            ("AssetData/InstanceProperties.bin", b"bad"),
        ]);
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
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
            .stream(&cfb_ctx, "RSeStorage/RSeSegInfo")
            .expect("lookup admission")
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
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(&zip);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(schema.len()) + 10;
        let (limited, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("package fits input limit");
        assert!(matches!(
            parse_stream(&limited, root),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "ZIP end record search"
        ));
        // This ZIP32 fixture has one EOCD, no comment, and no ZIP64 records.
        // The dependency bound is 128N * (1 candidate + 1 retry) = 256N.
        // Add N end-search work, one candidate, and three central headers.
        let zip_bytes = cadmpeg_core::decode::u64_from_index(zip.len());
        let inventory_work = 257 * zip_bytes + 4;
        let archive_name_lengths = [
            "Schemas/SimpleSchema.xml".len(),
            "First/InstanceProperties.bin".len(),
            "Second/InstanceProperties.bin".len(),
        ];
        let archive_name_bytes = archive_name_lengths.iter().copied().sum::<usize>();
        let archive_entry_count = archive_name_lengths.len();
        // A B-tree of h levels holds at least 2 * 6^(h-1) - 1 entries.
        let tree_height = |len: usize| {
            let mut height = 0;
            let mut minimum = 1;
            while len >= minimum {
                height += 1;
                minimum = 2 * 6_usize.pow(height) - 1;
            }
            usize::try_from(height).expect("fixture tree height fits usize")
        };
        // A key search compares at most eleven keys per level and at most every key.
        let tree_comparisons = |len: usize| (11 * tree_height(len)).min(len);
        // A tree of n entries holds at most (n - 1) / 5 + 1 nodes. Each new
        // B-tree key shifts one node, pays two passes for each node its length
        // adds to that bound, and makes two key searches.
        let node_bound = |len: usize| if len == 0 { 0 } else { (len - 1) / 5 + 1 };
        let btree_insert_work =
            |key_size: usize, value_size: usize, key_alignment: usize, value_alignment: usize| {
                archive_name_lengths
                    .iter()
                    .enumerate()
                    .map(|(len, key_bytes)| {
                        let alignment = key_alignment
                            .max(value_alignment)
                            .max(std::mem::align_of::<usize>());
                        let passes = 1 + 2 * (node_bound(len + 1) - node_bound(len));
                        let node_bytes = (key_size + value_size) * 11
                            + 16 * std::mem::size_of::<usize>()
                            + 2 * alignment;
                        let tree_mutation_work = node_bytes * passes;
                        let key_comparison_work = 2 * *key_bytes * tree_comparisons(len);
                        tree_mutation_work + key_comparison_work
                    })
                    .sum::<usize>()
            };
        let archive_tree_work = btree_insert_work(
            std::mem::size_of::<&[u8]>(),
            std::mem::size_of::<()>(),
            std::mem::align_of::<&[u8]>(),
            std::mem::align_of::<()>(),
        ) + btree_insert_work(
            std::mem::size_of::<String>(),
            std::mem::size_of::<()>(),
            std::mem::align_of::<String>(),
            std::mem::align_of::<()>(),
        ) + btree_insert_work(
            std::mem::size_of::<String>(),
            std::mem::size_of::<usize>(),
            std::mem::align_of::<String>(),
            std::mem::align_of::<usize>(),
        );
        // Each snapshot copies names into three owners and charges three
        // central-name, entry-record, and name-index visits.
        let archive_snapshot_work = cadmpeg_core::decode::u64_from_index(
            archive_tree_work + 3 * archive_name_bytes + 3 * archive_entry_count,
        );
        let archive_map_comparisons = tree_comparisons(archive_entry_count);
        // The schema lookup is inside its calibrated load; two instance opens
        // each look up one key in the three-entry Protein name map.
        let instance_open_lookup_work = cadmpeg_core::decode::u64_from_index(
            archive_name_lengths
                .iter()
                .skip(1)
                .map(|key_bytes| *key_bytes * archive_map_comparisons)
                .sum::<usize>(),
        );
        // Each of the three names is scanned once, every byte plus the end
        // probe.
        let archive_name_validation_work =
            cadmpeg_core::decode::u64_from_index(archive_name_bytes + 3);
        // Calibrate one complete schema load followed by both exact framing
        // and decode calls, using the same catalog as the production path.
        let schema_and_instance_decode_succeeds = |limit| {
            let arena = DecodeArena::new();
            let mut decode_policy = DecodePolicy::service();
            decode_policy.limits.max_work_units = limit;
            let (ctx, root) = DecodeContext::from_root_bytes(&zip, &arena, &decode_policy)
                .expect("schema ZIP fits input limit");
            let mut catalog = match cadmpeg_protein::SchemaCatalog::load(&ctx, root) {
                Ok(Some(catalog)) => catalog,
                Ok(None) => panic!("schema fixture loads an empty catalog"),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                    return false;
                }
                Err(error) => panic!("schema fixture failed below its work bound: {error}"),
            };
            for instance_bytes in [instance.as_slice(), instance.as_slice()] {
                let frames =
                    match cadmpeg_protein::framing::record_frames_admitted(&ctx, instance_bytes) {
                        Ok(frames) => frames,
                        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                            return false;
                        }
                        Err(error) => panic!("instance framing failed: {error}"),
                    };
                let outcome = match cadmpeg_protein::decode_frames_admitted(
                    &ctx,
                    &mut catalog,
                    frames.frames(),
                ) {
                    Ok(outcome) => outcome,
                    Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                        return false;
                    }
                    Err(error) => panic!("instance decoding failed: {error}"),
                };
                assert_eq!(outcome.records.len(), 1);
            }
            true
        };
        let mut decoded_path_lower = 0;
        let mut decoded_path_upper = DecodePolicy::service().limits.max_work_units;
        assert!(schema_and_instance_decode_succeeds(decoded_path_upper));
        while decoded_path_upper - decoded_path_lower > 1 {
            let middle = decoded_path_lower + (decoded_path_upper - decoded_path_lower) / 2;
            if schema_and_instance_decode_succeeds(middle) {
                decoded_path_upper = middle;
            } else {
                decoded_path_lower = middle;
            }
        }
        assert!(!schema_and_instance_decode_succeeds(decoded_path_lower));
        let schema_and_instance_decode_work = decoded_path_upper;
        // Instance entry names are copied into the returned records.
        let instance_entry_name_bytes =
            "First/InstanceProperties.bin".len() + "Second/InstanceProperties.bin".len();
        // Opening both stored instance entries charges their payload CRC scans.
        let instance_crc_work = 2 * cadmpeg_core::decode::u64_from_index(instance.len());
        // Validation and selection admit each entry. Both the filtered stream
        // collection and the fallible record collection charge two yields + end.
        let original_archive_entry_work =
            2 * cadmpeg_core::decode::u64_from_index(archive_entry_count) + 2 * (2 + 1);
        // Validation and the streaming decoder each visit every source entry
        // and one terminal probe. There is no selected-entry Vec or second
        // traversal over its two records.
        let archive_entry_work = 2 * (cadmpeg_core::decode::u64_from_index(archive_entry_count) + 1);
        // Counts the outer ZIP snapshot/name checks, one catalog load and both frame/decode paths, instance CRCs/lookups/name copies, and archive traversals/collections.
        let common_work = inventory_work
            + archive_snapshot_work
            + schema_and_instance_decode_work
            + instance_crc_work
            + cadmpeg_core::decode::u64_from_index(instance_entry_name_bytes)
            + archive_name_validation_work
            + instance_open_lookup_work;
        let original_work = common_work + original_archive_entry_work;
        let mut original_policy = DecodePolicy::service();
        original_policy.limits.max_work_units = original_work;
        let original_arena = DecodeArena::new();
        let (original_ctx, original_root) =
            DecodeContext::from_root_bytes(&bytes, &original_arena, &original_policy)
                .expect("package fits the service input limit");
        let ParsedProtein::Package {
            archive: original_archive,
            payload: original_payload,
            ..
        } = parse_stream(&original_ctx, original_root).expect("package parses")
        else {
            panic!("package state");
        };
        let original_instances = decode_instances_from(
            &original_ctx, &original_archive, original_payload,
        ).expect("the original allowance fits after removing both temporary traversals");
        assert_eq!(original_instances.len(), 2);
        assert!(original_instances.iter().all(|instance| instance.records.len() == 1));
        let remaining = original_archive_entry_work - archive_entry_work;
        let Err(cadmpeg_core::CodecError::ResourceLimit(original_limit)) =
            original_ctx.charge_work(remaining + 1, "probe remaining original Protein work")
        else {
            panic!("the removed traversals leave exactly their original allowance");
        };
        assert_eq!(original_limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original_limit.operation, "probe remaining original Protein work");
        assert_eq!(original_limit.used, common_work + archive_entry_work);
        assert_eq!(original_limit.additional, remaining + 1);
        assert!(matches!(
            original_ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == original_limit
        ));

        policy.limits.max_work_units = common_work + archive_entry_work;
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
        let mut bytes = (u32::try_from(zip.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
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
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    }

    fn paged_instance(record: &[u8]) -> Vec<u8> {
        const BODY_SIZE: usize = PAGE_SIZE - 8;
        let mut bytes = (u32::try_from(PAGE_SIZE).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
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
                bytes.extend_from_slice(
                    &(u16::try_from(chunk.len()).expect("fixture value fits u16")).to_le_bytes(),
                );
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
