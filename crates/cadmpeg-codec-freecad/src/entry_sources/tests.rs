// SPDX-License-Identifier: Apache-2.0
//! Source identity and navigation controls for archive entry interpretation.

use super::EntrySources;
use crate::native::EntryRecord;
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy, View};
use std::collections::BTreeMap;

fn entry(name: &str) -> EntryRecord {
    EntryRecord::new(
        crate::native::native_id("entry", name),
        name.into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn sources_keep_actual_named_member_views_without_copying_or_read_credit() {
    let bytes = b"prefixfirstsecond";
    let arena = DecodeArena::new();
    let (context, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let first = context
        .register_named_slice(root, ByteRange { start: 6, end: 11 }, "first.bin")
        .unwrap();
    let second = context
        .register_named_slice(root, ByteRange { start: 11, end: 17 }, "second.bin")
        .unwrap();
    let views = BTreeMap::from([("first.bin".into(), first), ("second.bin".into(), second)]);
    let sources = EntrySources::new(&context, &views).unwrap();
    for (name, expected, expected_len) in [
        ("first.bin", &bytes[6..11], 5),
        ("second.bin", &bytes[11..17], 6),
    ] {
        let record = entry(name);
        let view = sources.view(&record).unwrap();
        assert_eq!(view.window(), expected);
        assert_eq!(view.window().as_ptr(), expected.as_ptr());
        assert_eq!(sources.byte_len(&record).unwrap(), expected_len);
        let capture = context.capture_part_extent(view).unwrap();
        assert_eq!(capture.stream(), name);
        assert_eq!(capture.offset(), 0);
        let native = serde_json::to_value(record).unwrap();
        for field in ["data", "byte_len", "sha256", "length"] {
            assert!(native.get(field).is_none(), "{field}");
        }
    }
    assert!(context.take_record_captures().unwrap().is_empty());
}

#[test]
fn absent_entry_does_not_resolve_by_a_shared_native_identity_or_drop_a_sibling() {
    let bytes = b"real sibling";
    let arena = DecodeArena::new();
    let (context, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let views = BTreeMap::from([("real.bin".into(), root)]);
    let sources = EntrySources::new(&context, &views).unwrap();
    let actual = entry("real.bin");
    let mut wire = serde_json::to_value(&actual).unwrap();
    wire["name"] = serde_json::json!("missing.bin");
    let missing: EntryRecord = serde_json::from_value(wire).unwrap();
    assert_eq!(actual.id(), missing.id());
    assert!(sources.view(&missing).is_err());
    assert_eq!(sources.view(&actual).unwrap().window(), bytes);
}

#[test]
fn detached_and_foreign_views_cannot_supply_entry_source_provenance() {
    let bytes = b"member";
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let detached = BTreeMap::from([("member.bin".into(), View::over_retained(bytes))]);
    assert!(EntrySources::new(&context, &detached).is_err());
    let other_arena = DecodeArena::new();
    let (_other_context, foreign) =
        DecodeContext::from_root_bytes(bytes, &other_arena, &DecodePolicy::service()).unwrap();
    let foreign = BTreeMap::from([("member.bin".into(), foreign)]);
    assert!(EntrySources::new(&context, &foreign).is_err());
}

fn named_zip(raw: &[u8], flags: u16, extra: Vec<u8>, recovered: bool) -> Vec<u8> {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let placeholder = "x".repeat(raw.len());
    let mut entry = RawEntry::stored(&placeholder, b"payload").with_flags(flags);
    entry.extra = extra;
    let mut bytes = zip_fixture::archive(&[entry]);
    let fields = bytes
        .windows(raw.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == placeholder.as_bytes()).then_some(at))
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 2);
    for at in fields {
        bytes[at..at + raw.len()].copy_from_slice(raw);
    }
    if recovered {
        let central = bytes
            .windows(4)
            .position(|bytes| bytes == b"PK\x01\x02")
            .unwrap();
        bytes.truncate(central);
    }
    bytes
}

#[test]
fn entry_owner_continues_the_actual_header_record_and_rejects_a_foreign_session() {
    let bytes = named_zip(b"member.bin", 0, Vec::new(), false);
    let arena = DecodeArena::new();
    let (context, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = crate::container::parts::archive_source(&context, root).unwrap();
    let frame = archive.member_frames(&context).unwrap()[0].unwrap();
    let source = archive.entry_name_source(&archive.entries()[0]).unwrap();
    let ((payload, record), owner) = context
        .record_owner(
            &crate::native::native_id("entry", "member.bin"),
            frame.header(),
            || {
                let payload = archive.read_member_header(&context, frame)?;
                let name = archive.read_entry_name(&context, &archive.entries()[0])?;
                Ok((payload, entry(&name)))
            },
        )
        .unwrap();
    let payload = archive.open_member(&context, payload).unwrap();
    let owners = BTreeMap::from([(
        "member.bin".into(),
        std::rc::Rc::new(
            crate::container::parts::EntryOwner::new(
                &context,
                owner,
                std::rc::Rc::clone(&archive),
                0,
            )
            .unwrap(),
        ),
    )]);
    let views = BTreeMap::from([("member.bin".into(), payload)]);
    let sources = EntrySources::new(&context, &views)
        .unwrap()
        .with_owners(&owners);
    let owner = sources.owner(&record).unwrap();
    let other_arena = DecodeArena::new();
    let (other, foreign) =
        DecodeContext::from_root_bytes(b"foreign", &other_arena, &DecodePolicy::service()).unwrap();
    assert!(other.record_fragment(owner, foreign, || Ok(())).is_err());
    let value = context
        .record_fragment(owner, payload, || {
            Ok(payload.read_slice(payload.start()..payload.end()).unwrap())
        })
        .unwrap();
    assert_eq!(value, b"payload");
    assert!(sources.owner(&entry("missing.bin")).is_err());
    let capture = context.capture_part_extent(payload).unwrap();
    let captures = context.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), record.id());
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([
            (
                "file".into(),
                vec![
                    u64::try_from(frame.payload().start()).unwrap()
                        ..u64::try_from(frame.payload().end()).unwrap(),
                    u64::try_from(source.start()).unwrap()..u64::try_from(source.end()).unwrap(),
                ]
            ),
            (capture.stream().to_owned(), vec![0..7]),
        ])
    );
}

#[test]
fn entry_name_getter_uses_actual_zip_encoding_and_records_each_consumers_real_read() {
    let raw = b"orig.bin";
    let unicode = "水.bin";
    let mut extra = 0x7075_u16.to_le_bytes().to_vec();
    extra.extend_from_slice(&u16::try_from(5 + unicode.len()).unwrap().to_le_bytes());
    extra.push(1);
    extra.extend_from_slice(&cadmpeg_test_support::zip_fixture::crc32(raw).to_le_bytes());
    extra.extend_from_slice(unicode.as_bytes());
    for (raw, flags, extra, recovered, expected, selected) in [
        (
            b"caf\x82.bin".as_slice(),
            0,
            Vec::new(),
            false,
            "café.bin",
            false,
        ),
        (
            "café.bin".as_bytes(),
            0,
            Vec::new(),
            false,
            "café.bin",
            false,
        ),
        (
            b"caf\x82.bin".as_slice(),
            0x800,
            Vec::new(),
            false,
            "café.bin",
            false,
        ),
        (
            b"caf\xff.bin".as_slice(),
            0x800,
            Vec::new(),
            true,
            "caf�.bin",
            false,
        ),
        (raw.as_slice(), 0, extra, false, unicode, true),
    ] {
        let bytes = named_zip(raw, flags, extra, recovered);
        let arena = DecodeArena::new();
        let (context, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let archive = crate::container::parts::archive_source(&context, root).unwrap();
        let declaration = &archive.entries()[0];
        let source = archive.entry_name_source(declaration).unwrap();
        assert_eq!(source.window(), raw);
        let (record, owner) = context
            .record_owner(&crate::native::native_id("entry", expected), source, || {
                Ok(entry(&archive.read_entry_name(&context, declaration)?))
            })
            .unwrap();
        let supplier = crate::container::parts::EntryOwner::new(
            &context,
            owner,
            std::rc::Rc::clone(&archive),
            0,
        )
        .unwrap();
        let id = crate::native::native_id("application", "name-consumer");
        let actual = context
            .record(&id, source, || supplier.read_name(&context))
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(record.name(), actual);
        let other_arena = DecodeArena::new();
        let (other, _) =
            DecodeContext::from_root_bytes(&bytes, &other_arena, &DecodePolicy::service()).unwrap();
        assert!(supplier.read_name(&other).is_err());
        let start = u64::try_from(source.start()).unwrap();
        let end = u64::try_from(source.end()).unwrap();
        let mut expected_reads = vec![start..end];
        if selected {
            expected_reads.push(end + 5..end + 9 + u64::try_from(unicode.len()).unwrap());
        }
        let captures = context.take_record_captures().unwrap();
        assert_eq!(captures.len(), 2);
        for id in [record.id(), id.as_str()] {
            let capture = captures.iter().find(|capture| capture.id() == id).unwrap();
            assert_eq!(
                capture.reads(),
                &BTreeMap::from([("file".into(), expected_reads.clone())])
            );
        }
    }
}
