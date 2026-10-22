// SPDX-License-Identifier: Apache-2.0
//! ZIP member source identity, compressed lineage and exact read attribution.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, SourceOrigin};

use super::*;

#[test]
fn stored_entry_reads_map_only_the_same_bytes_in_the_original_payload() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let archive = ArchiveSnapshot::new(&ctx, root).unwrap();
    let entry = archive.entry("stored.bin").unwrap();
    let payload = entry.stored_range().unwrap();
    let view = archive.open(&ctx, "stored.bin").unwrap();
    let capture = ctx.capture_part_extent(view).unwrap();
    let name = capture.stream().to_owned();
    assert!(name.ends_with(":stored.bin"));
    assert_eq!((capture.offset(), capture.byte_len()), (0, 6));
    assert_eq!(view.window(), b"stored");
    assert_eq!(
        view.window().as_ptr(),
        root.child(
            usize::try_from(payload.start).unwrap(),
            usize::try_from(payload.end).unwrap()
        )
        .unwrap()
        .window()
        .as_ptr()
    );
    ctx.record("stored-value", view.child(1, 3).unwrap(), || {
        assert_eq!(view.read_at(1, 2), Some(b"to".as_slice()));
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([
            ("file".into(), vec![payload.start + 1..payload.start + 3]),
            (name.clone(), vec![1..3]),
        ])
    );
    let streams = ctx.source_streams().unwrap();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|stream| stream.name() == name
        && matches!(
            stream.origin(), SourceOrigin::Alias { parent, extent }
            if parent == "file" && *extent == payload
        )));
}

#[test]
fn deflate_and_zstd_reads_retain_the_actual_complete_compressed_parent_extent() {
    for (member, expected) in [
        ("deflated.bin", b"deflated payload".to_vec()),
        ("zstd.bin", zip_fixture::zstd_golden_plaintext()),
    ] {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let archive = ArchiveSnapshot::new(&ctx, root).unwrap();
        let payload = archive.entry(member).unwrap().stored_range().unwrap();
        let view = archive.open(&ctx, member).unwrap();
        assert_eq!(view.window(), expected.as_slice());
        let capture = ctx.capture_part_extent(view).unwrap();
        let name = capture.stream().to_owned();
        assert_eq!(capture.offset(), 0);
        ctx.record("expanded-value", view.child(1, 3).unwrap(), || {
            assert_eq!(view.read_at(1, 2), Some(&expected[1..3]));
            Ok(())
        })
        .unwrap();
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].reads(),
            &BTreeMap::from([
                ("file".into(), vec![payload.start..payload.end]),
                (name.clone(), vec![1..3]),
            ])
        );
        let streams = ctx.source_streams().unwrap();
        assert_eq!(streams.len(), 2);
        assert!(streams.iter().any(|stream| stream.name() == name
            && matches!(
                stream.origin(), SourceOrigin::Expanded { parent, extent }
                if parent == "file" && *extent == payload
            )));
    }
}

#[test]
fn opening_members_without_semantic_reads_creates_no_record_capture() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let archive = ArchiveSnapshot::new(&ctx, root).unwrap();
    for name in ["stored.bin", "deflated.bin", "zstd.bin"] {
        archive.open(&ctx, name).unwrap();
    }
    assert_eq!(ctx.source_streams().unwrap().len(), 4);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn repeated_opens_reuse_the_same_actual_source_stream_identity() {
    for member in ["stored.bin", "deflated.bin", "zstd.bin"] {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let first_archive = ArchiveSnapshot::new(&ctx, root).unwrap();
        let second_archive = ArchiveSnapshot::new(&ctx, root).unwrap();
        let first = first_archive.open(&ctx, member).unwrap();
        let second = second_archive.open(&ctx, member).unwrap();
        assert_eq!(first.window(), second.window());
        assert_eq!(first.location().space, second.location().space);
        assert_eq!(
            ctx.capture_part_extent(first).unwrap(),
            ctx.capture_part_extent(second).unwrap()
        );
        assert_eq!(ctx.source_streams().unwrap().len(), 2);
    }
}

#[test]
fn same_named_entries_in_different_real_archive_windows_have_distinct_provenance() {
    let zip = zip_fixture::archive(&[RawEntry::stored("payload", b"value")]);
    let mut bytes = b"prefix".to_vec();
    let first_start = bytes.len();
    bytes.extend_from_slice(&zip);
    let first_end = bytes.len();
    bytes.extend_from_slice(b"between");
    let second_start = bytes.len();
    bytes.extend_from_slice(&zip);
    let second_end = bytes.len();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let first_archive =
        ArchiveSnapshot::new(&ctx, root.child(first_start, first_end).unwrap()).unwrap();
    let second_archive =
        ArchiveSnapshot::new(&ctx, root.child(second_start, second_end).unwrap()).unwrap();
    let first = first_archive.open(&ctx, "payload").unwrap();
    let second = second_archive.open(&ctx, "payload").unwrap();
    let first_capture = ctx.capture_part_extent(first).unwrap();
    let second_capture = ctx.capture_part_extent(second).unwrap();
    assert_ne!(first_capture.stream(), second_capture.stream());
    assert_eq!(first.window(), b"value");
    assert_eq!(second.window(), b"value");
    for (archive, start, capture) in [
        (&first_archive, first_start, first_capture),
        (&second_archive, second_start, second_capture),
    ] {
        let relative = archive.entry("payload").unwrap().stored_range().unwrap();
        let expected = ByteRange {
            start: cadmpeg_core::decode::u64_from_index(start) + relative.start,
            end: cadmpeg_core::decode::u64_from_index(start) + relative.end,
        };
        assert!(ctx.source_streams().unwrap().iter().any(|stream| {
            stream.name() == capture.stream()
                && matches!(stream.origin(),
                SourceOrigin::Alias { parent, extent } if parent == "file" && *extent == expected)
        }));
    }
}

#[test]
fn nested_member_names_follow_the_real_host_stream_not_a_raw_name_collision() {
    let inner_bytes = zip_fixture::archive(&[RawEntry::stored("payload", b"inner")]);
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("inner.zip", &inner_bytes),
        RawEntry::stored("payload", b"outer"),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let archive = ArchiveSnapshot::new(&ctx, root).unwrap();
    let inner_view = archive.open(&ctx, "inner.zip").unwrap();
    let nested = ArchiveSnapshot::new(&ctx, inner_view).unwrap();
    let inner = nested.open(&ctx, "payload").unwrap();
    let outer = archive.open(&ctx, "payload").unwrap();
    let host_name = ctx
        .capture_part_extent(inner_view)
        .unwrap()
        .stream()
        .to_owned();
    let inner_name = ctx.capture_part_extent(inner).unwrap().stream().to_owned();
    let outer_name = ctx.capture_part_extent(outer).unwrap().stream().to_owned();
    assert_ne!(inner_name, outer_name);
    assert_eq!(inner.window(), b"inner");
    assert_eq!(outer.window(), b"outer");
    assert!(ctx.source_streams().unwrap().iter().any(|stream| {
        stream.name() == inner_name
            && matches!(stream.origin(),
            SourceOrigin::Alias { parent, extent } if parent == &host_name
                && *extent == nested.entry("payload").unwrap().stored_range().unwrap())
    }));
}

#[test]
fn opened_views_cannot_be_forged_from_detached_or_foreign_archive_roots() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let foreign_arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let (foreign_ctx, foreign_root) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::default()).unwrap();
    let foreign = ArchiveSnapshot::new(&foreign_ctx, foreign_root).unwrap();
    let detached = ArchiveSnapshot::new(&ctx, View::over_retained(&bytes)).unwrap();
    for name in ["stored.bin", "deflated.bin", "zstd.bin"] {
        assert!(foreign.open(&ctx, name).is_err());
        assert!(detached.open(&ctx, name).is_err());
    }
    assert_eq!(ctx.source_streams().unwrap().len(), 1);
}

#[test]
fn failed_member_checksums_do_not_register_validated_member_streams() {
    let plain = b"payload";
    let zstd = zip_fixture::zstd_stored_frame(plain);
    for mut entry in [
        RawEntry::stored("bad", plain),
        RawEntry::compressed("bad", zip_fixture::DEFLATE, &deflate(plain), plain),
        RawEntry::compressed("bad", zip_fixture::ZSTD, &zstd, plain),
    ] {
        entry.crc32 ^= 1;
        let bytes = zip_fixture::archive(&[entry]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let archive = ArchiveSnapshot::new(&ctx, root).unwrap();
        assert!(
            matches!(archive.open(&ctx, "bad"), Err(CodecError::Malformed(message)) if message == "CRC mismatch for bad")
        );
        assert_eq!(ctx.source_streams().unwrap().len(), 1);
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn stored_and_deflated_nonzero_archive_windows_keep_exact_source_coordinates() {
    for (member, expected, compressed) in [
        ("stored.bin", b"stored".as_slice(), false),
        ("deflated.bin", b"deflated payload".as_slice(), true),
    ] {
        let zip = archive_bytes();
        let mut bytes = b"outside-prefix".to_vec();
        let start = bytes.len();
        bytes.extend_from_slice(&zip);
        let end = bytes.len();
        bytes.extend_from_slice(b"outside-suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let enclosing = root.child(2, bytes.len() - 2).unwrap();
        let source = enclosing.child(start, end).unwrap();
        assert_ne!(source.start(), 0);
        let archive = ArchiveSnapshot::new(&ctx, source).unwrap();
        let relative = archive.entry(member).unwrap().stored_range().unwrap();
        let payload = ByteRange {
            start: cadmpeg_core::decode::u64_from_index(start) + relative.start,
            end: cadmpeg_core::decode::u64_from_index(start) + relative.end,
        };
        let view = archive.open(&ctx, member).unwrap();
        assert_eq!(view.window(), expected);
        let name = ctx.capture_part_extent(view).unwrap().stream().to_owned();
        let streams = ctx.source_streams().unwrap();
        let origin = streams
            .iter()
            .find(|stream| stream.name() == name)
            .unwrap()
            .origin();
        if compressed {
            assert!(matches!(origin, SourceOrigin::Expanded { parent, extent }
                if parent == "file" && *extent == payload));
        } else {
            assert!(matches!(origin, SourceOrigin::Alias { parent, extent }
                if parent == "file" && *extent == payload));
        }
        ctx.record("payload", view, || {
            assert_eq!(view.read_at(1, 2), Some(&expected[1..3]));
            Ok(())
        })
        .unwrap();
        let source_read = if compressed {
            payload.start..payload.end
        } else {
            payload.start + 1..payload.start + 3
        };
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].reads(),
            &BTreeMap::from([("file".into(), vec![source_read]), (name, vec![1..3]),])
        );
    }
}

#[test]
fn nested_nonzero_archive_windows_project_reads_through_the_real_stored_host() {
    for compressed in [false, true] {
        let expected = b"nested payload";
        let encoded = deflate(expected);
        let entry = if compressed {
            RawEntry::compressed("payload", zip_fixture::DEFLATE, &encoded, expected)
        } else {
            RawEntry::stored("payload", expected)
        };
        let inner = zip_fixture::archive(&[entry]);
        let mut host = b"host-prefix".to_vec();
        let inner_start = host.len();
        host.extend_from_slice(&inner);
        let inner_end = host.len();
        host.extend_from_slice(b"host-suffix");
        let bytes = zip_fixture::archive(&[RawEntry::stored("host", &host)]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let outer = ArchiveSnapshot::new(&ctx, root).unwrap();
        let outer_payload = outer.entry("host").unwrap().stored_range().unwrap();
        let host_view = outer.open(&ctx, "host").unwrap();
        let host_name = ctx
            .capture_part_extent(host_view)
            .unwrap()
            .stream()
            .to_owned();
        let source = host_view.child(inner_start, inner_end).unwrap();
        assert_ne!(source.start(), 0);
        let nested = ArchiveSnapshot::new(&ctx, source).unwrap();
        let relative = nested.entry("payload").unwrap().stored_range().unwrap();
        let payload = ByteRange {
            start: cadmpeg_core::decode::u64_from_index(inner_start) + relative.start,
            end: cadmpeg_core::decode::u64_from_index(inner_start) + relative.end,
        };
        let view = nested.open(&ctx, "payload").unwrap();
        assert_eq!(view.window(), expected);
        let name = ctx.capture_part_extent(view).unwrap().stream().to_owned();
        let streams = ctx.source_streams().unwrap();
        let origin = streams
            .iter()
            .find(|stream| stream.name() == name)
            .unwrap()
            .origin();
        if compressed {
            assert!(matches!(origin, SourceOrigin::Expanded { parent, extent }
                if parent == &host_name && *extent == payload));
        } else {
            assert!(matches!(origin, SourceOrigin::Alias { parent, extent }
                if parent == &host_name && *extent == payload));
        }
        ctx.record("nested-payload", view, || {
            assert_eq!(view.read_at(1, 2), Some(&expected[1..3]));
            Ok(())
        })
        .unwrap();
        let host_read = if compressed {
            payload.start..payload.end
        } else {
            payload.start + 1..payload.start + 3
        };
        let file_read = outer_payload.start + host_read.start..outer_payload.start + host_read.end;
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].reads(),
            &BTreeMap::from([
                ("file".into(), vec![file_read]),
                (host_name, vec![host_read]),
                (name, vec![1..3]),
            ])
        );
    }
}
