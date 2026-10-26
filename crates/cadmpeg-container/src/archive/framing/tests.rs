// SPDX-License-Identifier: Apache-2.0
//! Literal source-byte controls for ZIP framing and content separation.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_test_support::bytes::{put_u16, put_u32, put_u64};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};

use super::*;

fn ranges(views: &[View<'_>]) -> BTreeSet<(usize, usize)> {
    views
        .iter()
        .map(|view| (view.start(), view.end()))
        .collect()
}

fn covers(views: &[View<'_>], at: usize) -> bool {
    views
        .iter()
        .any(|view| view.start() <= at && at < view.end())
}

#[test]
fn framing_views_exclude_content_and_do_not_read_or_rebase_nonzero_sources() {
    let mut entry = RawEntry::stored("entry", b"payload");
    entry.extra = vec![0x34, 0x12, 3, 0, b'a', b'b', b'c'];
    let mut zip = zip_fixture::archive(&[entry]);
    let central = 30 + 5 + 7 + 7;
    let end = central + 46 + 5 + 7;
    put_u16(&mut zip, end + 20, 4);
    zip.extend_from_slice(b"note");
    let mut bytes = b"outside prefix".to_vec();
    let origin = bytes.len();
    bytes.extend_from_slice(&zip);
    let source_end = bytes.len();
    bytes.extend_from_slice(b"outside suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let source = root.child(origin, source_end).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, source).unwrap();
    let views = ctx
        .record("navigation", source, || snapshot.framing_views(&ctx))
        .unwrap();
    let expected = [
        (0, 4),
        (6, 8),
        (18, 22),
        (26, 30),
        (35, 39),
        (central, central + 4),
        (central + 8, central + 10),
        (central + 20, central + 34),
        (central + 42, central + 46),
        (central + 51, central + 55),
        (end, end + 22),
    ]
    .map(|(start, end)| (origin + start, origin + end));
    assert_eq!(ranges(&views), BTreeSet::from(expected));
    for view in &views {
        assert_eq!(
            view.location(),
            root.child(view.start(), view.end()).unwrap().location()
        );
        assert_eq!(
            view.window().as_ptr(),
            root.child(view.start(), view.end())
                .unwrap()
                .window()
                .as_ptr()
        );
        let capture = ctx.capture_part_extent(*view).unwrap();
        assert_eq!(capture.stream(), "file");
        assert_eq!(capture.offset(), u64_from_index(view.start()));
    }
    assert_eq!(ctx.source_streams().unwrap().len(), 1);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn descriptor_framing_keeps_both_sizes_but_excludes_the_crc_at_every_width() {
    for wide in [false, true] {
        for signed in [false, true] {
            let mut entry = RawEntry::stored("e", b"data").with_flags(8);
            if wide {
                entry.extra.extend_from_slice(&[1, 0, 16, 0]);
                entry.extra.extend_from_slice(&4_u64.to_le_bytes());
                entry.extra.extend_from_slice(&4_u64.to_le_bytes());
            }
            let descriptor_start = 30 + 1 + entry.extra.len() + 4;
            let crc = entry.crc32;
            let mut bytes = zip_fixture::archive(&[entry]);
            let mut descriptor = Vec::new();
            if signed {
                descriptor.extend_from_slice(b"PK\x07\x08");
            }
            descriptor.extend_from_slice(&crc.to_le_bytes());
            if wide {
                descriptor.extend_from_slice(&4_u64.to_le_bytes());
                descriptor.extend_from_slice(&4_u64.to_le_bytes());
                put_u32(&mut bytes, 18, u32::MAX);
                put_u32(&mut bytes, 22, u32::MAX);
            } else {
                descriptor.extend_from_slice(&4_u32.to_le_bytes());
                descriptor.extend_from_slice(&4_u32.to_le_bytes());
            }
            bytes.splice(
                descriptor_start..descriptor_start,
                descriptor.iter().copied(),
            );
            let end = bytes.len() - 22;
            put_u32(
                &mut bytes,
                end + 16,
                u32::try_from(descriptor_start + descriptor.len()).unwrap(),
            );
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
            let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
            let views = snapshot.framing_views(&ctx).unwrap();
            let prefix = if signed { 4 } else { 0 };
            for at in descriptor_start..descriptor_start + prefix {
                assert!(covers(&views, at));
            }
            for at in descriptor_start + prefix..descriptor_start + prefix + 4 {
                assert!(!covers(&views, at));
            }
            for at in descriptor_start + prefix + 4..descriptor_start + descriptor.len() {
                assert!(covers(&views, at));
            }
            assert_eq!(snapshot.open(&ctx, "e").unwrap().window(), b"data");
        }
    }
}

#[test]
fn zip64_scalar_words_are_framing_but_extra_and_footer_extensions_are_content() {
    let mut entry = RawEntry::stored("e", b"data");
    entry.extra.extend_from_slice(&[1, 0, 24, 0]);
    entry.extra.extend_from_slice(&4_u64.to_le_bytes());
    entry.extra.extend_from_slice(&4_u64.to_le_bytes());
    entry.extra.extend_from_slice(&0_u64.to_le_bytes());
    let central = 30 + 1 + entry.extra.len() + 4;
    let central_size = 46 + 1 + entry.extra.len();
    let mut bytes = zip_fixture::archive(&[entry]);
    put_u32(&mut bytes, 18, u32::MAX);
    put_u32(&mut bytes, 22, u32::MAX);
    for offset in [20, 24, 42] {
        put_u32(&mut bytes, central + offset, u32::MAX);
    }
    let old_end = bytes.split_off(bytes.len() - 22);
    let record = bytes.len();
    bytes.resize(record + 60 + 20, 0);
    put_u32(&mut bytes, record, 0x0606_4b50);
    put_u64(&mut bytes, record + 4, 48);
    put_u16(&mut bytes, record + 12, 45);
    put_u16(&mut bytes, record + 14, 45);
    put_u64(&mut bytes, record + 24, 1);
    put_u64(&mut bytes, record + 32, 1);
    put_u64(&mut bytes, record + 40, u64_from_index(central_size));
    put_u64(&mut bytes, record + 48, u64_from_index(central));
    bytes[record + 56..record + 60].copy_from_slice(b"TAIL");
    let locator = record + 60;
    put_u32(&mut bytes, locator, 0x0706_4b50);
    put_u64(&mut bytes, locator + 8, u64_from_index(record));
    put_u32(&mut bytes, locator + 16, 1);
    let end = bytes.len();
    bytes.extend_from_slice(&old_end);
    put_u16(&mut bytes, end + 8, u16::MAX);
    put_u16(&mut bytes, end + 10, u16::MAX);
    put_u32(&mut bytes, end + 12, u32::MAX);
    put_u32(&mut bytes, end + 16, u32::MAX);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
    let views = snapshot.framing_views(&ctx).unwrap();
    for at in 31..31 + 4 + 16 {
        assert!(covers(&views, at));
    }
    for at in 31 + 4 + 16..31 + 4 + 24 {
        assert!(!covers(&views, at));
    }
    for at in central + 47..central + 47 + 4 + 24 {
        assert!(covers(&views, at));
    }
    for at in record..record + 56 {
        assert!(covers(&views, at));
    }
    for at in record + 56..locator {
        assert!(!covers(&views, at));
    }
    for at in locator..end + 22 {
        assert!(covers(&views, at));
    }
    assert_eq!(snapshot.open(&ctx, "e").unwrap().window(), b"data");
}

#[test]
fn digital_signature_data_and_archive_comments_remain_content() {
    for inside in [false, true] {
        let mut bytes = zip_fixture::archive(&[RawEntry::stored("e", b"data")]);
        let signature = bytes.len() - 22;
        bytes.splice(signature..signature, *b"PK\x05\x05\x03\x00abc");
        let end = signature + 9;
        if inside {
            put_u32(&mut bytes, end + 12, 47 + 9);
        }
        put_u16(&mut bytes, end + 20, 4);
        bytes.extend_from_slice(b"note");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
        let views = snapshot.framing_views(&ctx).unwrap();
        for at in signature..signature + 6 {
            assert!(covers(&views, at));
        }
        for at in signature + 6..end {
            assert!(!covers(&views, at));
        }
        for at in end..end + 22 {
            assert!(covers(&views, at));
        }
        for at in end + 22..bytes.len() {
            assert!(!covers(&views, at));
        }
    }
}

#[test]
fn recovered_local_headers_do_not_invent_footer_or_payload_framing() {
    let complete = zip_fixture::archive(&[RawEntry::stored("e", b"data")]);
    for cut in [33, 35] {
        let bytes = &complete[..cut];
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
        assert!(snapshot.recovery_reason().is_some());
        let views = snapshot.framing_views(&ctx).unwrap();
        assert_eq!(
            ranges(&views),
            BTreeSet::from([(0, 4), (6, 8), (18, 22), (26, 30)])
        );
        for at in 30..cut {
            assert!(!covers(&views, at));
        }
    }
}

#[test]
fn framing_rejects_foreign_sources_and_uses_the_original_retained_budget() {
    let bytes = zip_fixture::archive(&[RawEntry::stored("e", b"data")]);
    let arena = DecodeArena::new();
    let foreign_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let (foreign, _) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::default()).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
    assert!(snapshot.framing_views(&foreign).is_err());
    let detached = ArchiveSnapshot::new(&ctx, View::over_retained(&bytes)).unwrap();
    assert!(detached.framing_views(&ctx).is_err());
    // #214 working-storage rule: admit the genuine source infrastructure before
    // filling retained output, then borrowed framing scratch must still work.
    ctx.framing_part_extent(root).unwrap();
    ctx.retain_bytes(
        ctx.retained_bytes_available(),
        "exhaust retained framing storage",
    )
    .unwrap();
    let views = snapshot.framing_views(&ctx).unwrap();
    assert_eq!(
        ranges(&views),
        BTreeSet::from([
            (0, 4),
            (6, 8),
            (18, 22),
            (26, 30),
            (35, 39),
            (43, 45),
            (55, 69),
            (77, 81),
            (82, 104),
        ])
    );
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(ctx.resource_refusal().is_none());
    drop(views);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(matches!(
        ctx.retain_bytes(1, "real retained output"),
        Err(CodecError::ResourceLimit(_))
    ));
    let first = ctx.resource_refusal().unwrap();
    assert!(
        matches!(snapshot.framing_views(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == first)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first)
    );
}

#[test]
fn framing_working_vectors_preserve_full_retained_output_while_live_and_after_drop() {
    let zip = zip_fixture::archive(&[RawEntry::stored("e", b"data")]);
    let mut bytes = b"pre".to_vec();
    bytes.extend_from_slice(&zip);
    let end = bytes.len();
    bytes.extend_from_slice(b"tail");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let source = root.child(3, end).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, source).unwrap();
    ctx.framing_part_extent(source).unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "actual held output")
        .unwrap();
    let views = snapshot.framing_views(&ctx).unwrap();
    let expected = [
        (0, 4),
        (6, 8),
        (18, 22),
        (26, 30),
        (35, 39),
        (43, 45),
        (55, 69),
        (77, 81),
        (82, 104),
    ]
    .map(|(start, end)| (3 + start, 3 + end));
    assert_eq!(ranges(&views), BTreeSet::from(expected));
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(ctx.resource_refusal().is_none());
    for view in &views {
        assert_eq!(
            view.window().as_ptr(),
            root.child(view.start(), view.end())
                .unwrap()
                .window()
                .as_ptr()
        );
    }
    drop(views);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(ctx.resource_refusal().is_none());
    assert!(ctx.take_record_captures().unwrap().is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn framing_working_storage_preserves_the_actual_caller_first_refusal() {
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure};
    let bytes = zip_fixture::archive(&[RawEntry::stored("e", b"data")]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let snapshot = ArchiveSnapshot::new(&ctx, root).unwrap();
    ctx.framing_part_extent(root).unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "actual held output")
        .unwrap();
    let CodecError::ResourceLimit(first) = ctx.retain_bytes(1, "real retained output").unwrap_err()
    else {
        panic!("actual retained ceiling")
    };
    assert_eq!(first.scope, LimitScope::Session);
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.reason, ResourceFailure::BudgetExceeded);
    assert_eq!(first.limit, 4096);
    assert_eq!(first.used, 4096);
    assert_eq!(first.additional, 1);
    assert_eq!(first.operation, "real retained output");
    assert!(
        matches!(snapshot.framing_views(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == first)
    );
    assert_eq!(ctx.resource_refusal(), Some(first));
    drop(snapshot);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first)
    );
}
