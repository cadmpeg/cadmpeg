// SPDX-License-Identifier: Apache-2.0
use super::{archive_bytes, ArchiveSnapshot, Cursor, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn expected(bytes: &[u8], target: &str) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    (0..archive.len()).any(|index| {
        archive
            .by_index(index)
            .is_ok_and(|entry| entry.name() == target)
    })
}

#[test]
fn readable_name_probe_matches_readable_entries_and_skips_bad_local_headers() {
    for broken in [false, true] {
        let mut bytes = archive_bytes();
        if broken {
            bytes[..4].fill(0);
        }
        for target in ["stored.bin", "deflated.bin", "zstd.bin", "absent"] {
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                    .expect("context");
            let found = ArchiveSnapshot::probe_readable_names(&ctx, root, |name| {
                ctx.equal(name, target, "probe comparison")
            })
            .expect("probe");
            assert_eq!(found, expected(&bytes, target));
        }
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"not a ZIP", &arena, &DecodePolicy::service())
            .expect("context");
    assert!(
        !ArchiveSnapshot::probe_readable_names(&ctx, root, |_| panic!(
            "invalid archive has no callback"
        ))
        .expect("invalid is false")
    );
}

#[test]
fn readable_name_probe_skips_encrypted_and_unsupported_members() {
    for (offset, value) in [(8, 1_u16), (10, 12_u16)] {
        let mut bytes = archive_bytes();
        let directory = bytes
            .windows(4)
            .position(|bytes| bytes == b"PK\x01\x02")
            .expect("central header");
        bytes[directory + offset..directory + offset + 2].copy_from_slice(&value.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("context");
        let found = ArchiveSnapshot::probe_readable_names(&ctx, root, |name| {
            ctx.equal(name, "stored.bin", "probe comparison")
        })
        .expect("probe");
        assert_eq!(found, expected(&bytes, "stored.bin"));
        assert!(!found);
    }
}

#[test]
fn readable_name_probe_keeps_child_refusal_and_short_circuits() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let calls = std::cell::Cell::new(0);
    assert!(ArchiveSnapshot::probe_readable_names(&ctx, root, |_| {
        calls.set(calls.get() + 1);
        Ok(true)
    })
    .expect("match"));
    assert_eq!(calls.get(), 1);
    let CodecError::ResourceLimit(first) =
        ArchiveSnapshot::probe_readable_names(&ctx, root, |_| {
            Err(ctx.refuse_codec_limit("predicate", 0, 1))
        })
        .expect_err("predicate refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first.operation, "predicate");
    let CodecError::ResourceLimit(repeated) =
        ArchiveSnapshot::probe_readable_names(&ctx, root, |_| Ok(true))
            .expect_err("original refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first, repeated);
}

#[test]
fn readable_name_probe_refuses_before_callback() {
    let bytes = archive_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let called = std::cell::Cell::new(false);
    assert!(matches!(
        ArchiveSnapshot::probe_readable_names(&ctx, root, |_| {
            called.set(true);
            Ok(true)
        }),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(!called.get());
}

#[test]
fn readable_name_probe_preserves_duplicate_name_behavior() {
    use super::SimpleFileOptions;
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["a.xml", "b.xml"] {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .expect("entry");
        writer.write_all(b"schema").expect("payload");
    }
    let mut bytes = writer.finish().expect("archive").into_inner();
    let offsets: Vec<_> = bytes
        .windows(5)
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == b"b.xml").then_some(index))
        .collect();
    assert_eq!(offsets.len(), 2);
    for offset in offsets {
        bytes[offset..offset + 5].copy_from_slice(b"a.xml");
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let found = ArchiveSnapshot::probe_readable_names(&ctx, root, |name| {
        ctx.equal(name, "a.xml", "duplicate probe")
    })
    .expect("tolerant probe");
    assert!(found);
    assert_eq!(found, expected(&bytes, "a.xml"));
    let Err(CodecError::Malformed(message)) = ArchiveSnapshot::new(&ctx, root) else {
        panic!("strict snapshot rejects duplicate names")
    };
    assert!(message.contains("duplicate"));
}
