// SPDX-License-Identifier: Apache-2.0
//! Inventor Protein package framing and inventory.

use std::num::NonZeroU32;

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::{ArchiveSnapshot, EntryRecord};
use cadmpeg_core::decode::{u64_from_index, ArenaClass, ByteRange, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::unknown::part::outcome::PartOutcome;
use cadmpeg_ir::unknown::part::{Cause, GivenUpPart, Part};

use crate::container::streams::Streams;
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
    streams: &mut Streams<'a>,
) -> Result<ProteinState<'a>, CodecError> {
    let Some(stream) = snapshot.stream("Protein") else {
        return Ok(ProteinState::Absent);
    };
    Ok(
        match streams.read(ctx, snapshot, stream, "protein-stream", |source| {
            parse_stream(ctx, source)
        }) {
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
        },
    )
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
    Ok(ParsedProtein::Package {
        declared_len,
        archive,
        payload,
    })
}

pub(crate) fn fuzz_parse_stream(ctx: &DecodeContext<'_>, source: View<'_>) {
    let _probe = parse_stream(ctx, source);
}

#[derive(Default)]
pub(crate) struct ProteinInstances<'a> {
    pub(crate) members: Vec<ProteinInstanceRecords>,
    outcomes: Vec<PartOutcome<'a>>,
}

impl ProteinInstances<'_> {
    pub(crate) fn give_ups(&self, ctx: &DecodeContext<'_>) -> Result<Vec<GivenUpPart>, CodecError> {
        let mut parts = Vec::new();
        for outcome in &self.outcomes {
            parts.extend(outcome.give_up(ctx, true)?);
        }
        Ok(parts)
    }
}

impl<'a> ProteinState<'a> {
    pub(crate) fn read_instances(
        &self,
        ctx: &DecodeContext<'a>,
    ) -> Result<ProteinInstances<'a>, CodecError> {
        match self {
            Self::Package(package) => decode_instances_from(ctx, &package.archive, package.payload),
            Self::Absent | Self::Empty { .. } | Self::Malformed { .. } => {
                Ok(ProteinInstances::default())
            }
        }
    }
}

pub(crate) fn entry_id(ordinal: usize) -> String {
    format!("inventor:protein:entry#{ordinal}")
}

fn decode_instances_from<'a>(
    ctx: &DecodeContext<'a>,
    archive: &ArchiveSnapshot<'a>,
    payload: View<'a>,
) -> Result<ProteinInstances<'a>, CodecError> {
    let mut output = ProteinInstances::default();
    if let Some(reason) = archive.recovery_reason() {
        let (offset, bytes) = archive
            .recovery_tail()
            .and_then(|range| member_bytes(payload, range))
            .unwrap_or((u64_from_index(payload.window().len()), &[]));
        let part = Part::at(
            "inventor",
            "protein-directory",
            offset,
            bytes,
            [ArenaClass::Model],
        )
        .in_stream(cadmpeg_ir::stream_name!("inventor:Protein:zip"));
        output
            .outcomes
            .push(PartOutcome::damaged(part, &CodecError::malformed(reason)));
    }
    let mut catalog = cadmpeg_protein::SchemaCatalog::default();
    let members = archive.entries().iter().enumerate();
    for (ordinal, entry) in members
        .clone()
        .filter(|(_, entry)| cadmpeg_protein::is_schema_entry(&entry.name))
    {
        let (outcome, _) = read_member(ctx, archive, payload, ordinal, entry, || {
            catalog.read_member(ctx, archive, entry).map(Some)
        })?;
        output.outcomes.push(outcome);
    }
    for (ordinal, entry) in
        members.filter(|(_, entry)| !cadmpeg_protein::is_schema_entry(&entry.name))
    {
        let (outcome, decoded) = read_member(ctx, archive, payload, ordinal, entry, || {
            if !entry.name.ends_with("InstanceProperties.bin") {
                return Ok(None);
            }
            let instance = archive.open(ctx, &entry.name)?;
            let frames = cadmpeg_protein::framing::record_frames(instance.window())?;
            if catalog.is_empty() {
                return Ok(None);
            }
            cadmpeg_protein::decode_frames(ctx, &mut catalog, &frames).map(Some)
        })?;
        output.outcomes.push(outcome);
        if let Some(decoded) = decoded {
            output.members.push(ProteinInstanceRecords {
                entry_name: entry.name.clone(),
                records: decoded.records,
                rejected: decoded.rejected,
            });
        }
    }
    Ok(output)
}

fn read_member<'a, T>(
    ctx: &DecodeContext<'a>,
    archive: &ArchiveSnapshot<'a>,
    payload: View<'a>,
    ordinal: usize,
    entry: &EntryRecord,
    read: impl FnOnce() -> Result<Option<T>, CodecError>,
) -> Result<(PartOutcome<'a>, Option<T>), CodecError> {
    let framing = archive.validate_member(&entry.name);
    let namespace = validate_entry_name(&entry.name);
    let part = member_part(
        payload,
        ordinal,
        entry,
        framing.is_ok() && namespace.is_ok(),
    )?;
    let (outcome, value) = if let Err(error) = framing {
        (PartOutcome::damaged(part, &error), None)
    } else {
        match ctx.run_part(|| {
            namespace?;
            read()
        })? {
            Ok(Some(value)) => (PartOutcome::admitted(part), Some(value)),
            Ok(None) => (PartOutcome::inventory(part), None),
            Err(error) => (PartOutcome::refused(part, Cause::Unreadable, &error), None),
        }
    };
    Ok((outcome, value))
}

fn member_part<'a>(
    payload: View<'a>,
    ordinal: usize,
    entry: &EntryRecord,
    use_payload: bool,
) -> Result<Part<'a>, CodecError> {
    let declaration = entry.declaration_range();
    let stored = if use_payload {
        entry.stored_range().ok()
    } else {
        None
    };
    let (offset, bytes) = stored
        .and_then(|span| member_bytes(payload, span))
        .or_else(|| member_bytes(payload, declaration))
        .unwrap_or((declaration.start, &[]));
    Ok(Part::at(
        "inventor",
        "protein-member",
        offset,
        bytes,
        [ArenaClass::Model],
    )
    .named(&entry.name)
    .in_stream(cadmpeg_ir::stream_name!("inventor:Protein:zip"))
    .contained_in(Identity::new(entry_id(ordinal))?)
    .digest_only())
}

fn member_bytes(payload: View<'_>, span: ByteRange) -> Option<(u64, &[u8])> {
    let start = usize::try_from(span.start).ok()?;
    let end = usize::try_from(span.end).ok()?;
    Some((span.start, payload.window().get(start..end)?))
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
    use crate::test_support::test_fixtures::{paged_instance, push_lp};
    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_test_support::zip_fixture;

    use super::{decode_instances_from, parse_stream, validate_entry_name, ParsedProtein};
    use cadmpeg_core::decode::DecodeContext;

    #[test]
    fn unsafe_protein_entry_name_is_rejected() {
        let error = validate_entry_name("../escape").expect_err("unsafe path rejected");
        assert!(error.to_string().contains("../escape"));
    }

    #[test]
    fn malformed_protein_remains_an_inventory_state() {
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
        assert!(matches!(
            super::parse(&setup, &snapshot, &mut super::Streams::default())
                .expect("malformed Protein remains an inventory state"),
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
        let zip = zip_entries(&[("Schemas/ExampleSchema.xml", b"synthetic")]);
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
    fn protein_rejects_length_mismatch_before_indexing() {
        let mut mismatch = 5_u32.to_le_bytes().to_vec();
        mismatch.extend_from_slice(b"four");
        with_stream(&mismatch, |ctx, root| {
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
            let instances = decode_instances_from(ctx, &archive, payload)
                .expect("instances decode")
                .members;
            assert_eq!(instances.len(), 1);
            assert_eq!(instances[0].records.len(), 1);
            assert_eq!(instances[0].records[0].schema, "SimpleSchema");
            assert_eq!(instances[0].records[0].guid, "asset-guid");
            assert!(instances[0].rejected.is_empty());
        });
    }

    #[test]
    fn protein_instance_records_decode() {
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
                archive, payload, ..
            } = parse_stream(ctx, root).expect("synthetic Protein package parses")
            else {
                panic!("package state")
            };
            assert_eq!(
                decode_instances_from(ctx, &archive, payload)
                    .expect("decode the instances")
                    .members
                    .len(),
                1
            );
        });
    }

    #[test]
    fn protein_semantic_issue_is_retained() {
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
            archive, payload, ..
        } = parse_stream(&setup, root).expect("ZIP package parses")
        else {
            panic!("package state");
        };
        let result = decode_instances_from(&setup, &archive, payload)
            .expect("a failed ZIP member leaves the package readable");
        assert!(result.members.is_empty());
        let parts = result.give_ups(&setup).expect("member reports");
        let [issue] = parts.as_slice() else {
            panic!("one unreadable ZIP member");
        };
        assert_eq!(
            issue.loss().code,
            cadmpeg_ir::unknown::part::Cause::Unreadable.kind()
        );
    }

    #[test]
    fn protein_instance_past_the_zstd_window_allowance_is_an_issue_without_a_refusal() {
        let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
        // The instance stream is a streamed frame as a level-22 encoder writes it: a
        // 128 MiB window and no content size. Laid out by `zip_fixture`: `zip` is built
        // without its `zstd` feature.
        let zip = zip_fixture::archive(&[
            zip_fixture::RawEntry::stored("Schemas/SimpleSchema.xml", schema),
            zip_fixture::RawEntry::compressed(
                "AssetData/InstanceProperties.bin",
                zip_fixture::ZSTD,
                &zip_fixture::zstd_streamed_frame(27, b"instance"),
                b"instance",
            ),
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
            archive, payload, ..
        } = parse_stream(&setup, root).expect("ZIP package parses")
        else {
            panic!("package state");
        };
        let result = decode_instances_from(&setup, &archive, payload)
            .expect("a failed ZIP member leaves the package readable");
        assert!(result.members.is_empty());
        let parts = result.give_ups(&setup).expect("member reports");
        let [issue] = parts.as_slice() else {
            panic!("one undecoded ZIP member");
        };
        let issue = issue.loss();
        assert_eq!(
            issue.code,
            cadmpeg_ir::unknown::part::Cause::Undecoded.kind()
        );
        assert!(
            issue.message.contains("per-expansion allowance"),
            "{issue:?}"
        );
        assert_eq!(setup.resource_refusal(), None);
        assert!(setup.finish_session().is_ok());
    }

    #[test]
    fn inventor_reuses_one_schema_catalog_across_instance_streams() {
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
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("package fits the service input limit");
        let ParsedProtein::Package {
            archive, payload, ..
        } = parse_stream(&ctx, root).expect("package parses")
        else {
            panic!("package state")
        };
        let instances = decode_instances_from(&ctx, &archive, payload)
            .expect("one schema catalog decodes both streams")
            .members;
        assert_eq!(instances.len(), 2);
        assert!(instances.iter().all(|instance| instance.records.len() == 1));
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
            let instances = decode_instances_from(ctx, &archive, payload)
                .expect("instances decode")
                .members;
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

    fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        zip_fixture::archive(
            &entries
                .iter()
                .map(|(name, bytes)| zip_fixture::RawEntry::stored(name, bytes))
                .collect::<Vec<_>>(),
        )
    }
}
