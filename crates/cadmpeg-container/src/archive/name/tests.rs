// SPDX-License-Identifier: Apache-2.0
use super::super::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};

fn fixture(raw: &[u8], flags: u16, extra: Vec<u8>, recovered: bool) -> Vec<u8> {
    let mut entry = RawEntry::stored(&"x".repeat(raw.len()), b"payload").with_flags(flags);
    entry.extra = extra;
    let mut bytes = zip_fixture::archive(&[entry]);
    let central = bytes
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .expect("fixture central directory signature");
    bytes[30..30 + raw.len()].copy_from_slice(raw);
    bytes[central + 46..central + 46 + raw.len()].copy_from_slice(raw);
    if recovered {
        bytes.truncate(central);
    }
    bytes
}

fn unicode(raw: &[u8], value: &[u8], valid_crc: bool) -> Vec<u8> {
    let mut extra = Vec::new();
    extra.extend_from_slice(&0x7075u16.to_le_bytes());
    extra.extend_from_slice(
        &u16::try_from(value.len() + 5)
            .expect("Unicode path extra length fits u16")
            .to_le_bytes(),
    );
    extra.push(7);
    extra.extend_from_slice(&(crc32fast::hash(raw) ^ u32::from(!valid_crc)).to_le_bytes());
    extra.extend_from_slice(value);
    extra
}

fn check(bytes: &[u8], raw: &[u8], expected: &str, extra_read: Option<(usize, usize)>) {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("actual source session");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("admitted source archive");
    let entry = &archive.entries()[0];
    assert_eq!(entry.name, expected);
    let source = archive
        .entry_name_source(entry)
        .expect("actual entry filename field");
    assert_eq!(source.window(), raw);
    assert_eq!(source.end() - source.start(), raw.len());
    let name = ctx
        .record("unrelated:test:entry#original", root, || {
            archive.read_entry_name(&ctx, entry)
        })
        .expect("commit actual filename read");
    assert_eq!(name, expected);
    let captures = ctx
        .take_record_captures()
        .expect("actual filename captures");
    assert_eq!(captures.len(), 1);
    let start = u64::try_from(source.start()).expect("filename start fits u64");
    let end = u64::try_from(source.end()).expect("filename end fits u64");
    let mut reads = std::iter::once(start..end).collect::<Vec<std::ops::Range<u64>>>();
    if let Some((offset, len)) = extra_read {
        let offset = end + u64::try_from(offset).expect("selected extra offset fits u64");
        reads.push(offset..offset + u64::try_from(len).expect("selected extra length fits u64"));
    }
    assert_eq!(captures[0].reads().len(), 1);
    assert_eq!(captures[0].reads()["file"], reads);
}

#[test]
fn central_names_preserve_utf8_first_and_cp437_without_flag_reinterpretation() {
    for flags in [0, 0x800] {
        check(
            &fixture("é".as_bytes(), flags, Vec::new(), false),
            "é".as_bytes(),
            "é",
            None,
        );
        check(
            &fixture(&[0x82], flags, Vec::new(), false),
            &[0x82],
            "é",
            None,
        );
    }
}

#[test]
fn central_unicode_path_uses_last_valid_original_checksum_and_exact_selected_fields() {
    let raw = b"original";
    let mut extra = unicode(raw, "first".as_bytes(), true);
    let before = extra.len();
    extra.extend(unicode(raw, "水".as_bytes(), true));
    extra.extend(unicode(raw, b"wrong", false));
    extra.extend(unicode(raw, &[0xff], true));
    check(
        &fixture(raw, 0, extra, false),
        raw,
        "水",
        Some((before + 5, 4 + "水".len())),
    );
}

#[test]
fn invalid_unicode_paths_leave_original_name_and_read_only_original_field() {
    let raw = &[0x82];
    let mut extra = unicode(raw, b"wrong", false);
    extra.extend(unicode(raw, &[0xff], true));
    check(&fixture(raw, 0, extra, false), raw, "é", None);
}

#[test]
fn local_recovery_keeps_flag_specific_lossy_utf8_and_ignores_unicode_extras() {
    check(
        &fixture(&[0xff], 0x800, Vec::new(), true),
        &[0xff],
        "�",
        None,
    );
    check(
        &fixture("é".as_bytes(), 0, Vec::new(), true),
        "é".as_bytes(),
        "├⌐",
        None,
    );
    let raw = &[0x82];
    check(
        &fixture(raw, 0, unicode(raw, "水".as_bytes(), true), true),
        raw,
        "é",
        None,
    );
}

#[test]
fn foreign_copied_and_tampered_entry_handles_fail_before_any_name_read() {
    let bytes = fixture(b"original", 0, Vec::new(), false);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual handle source session");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("actual handle archive");
    let foreign_arena = DecodeArena::new();
    let (foreign_ctx, foreign_root) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service())
            .expect("foreign handle source session");
    let foreign = ArchiveSnapshot::new(&foreign_ctx, foreign_root).expect("foreign handle archive");
    let copied = archive.entries()[0].clone();
    let mut tampered = archive.entries()[0].clone();
    tampered.name.push_str("-tampered");
    ctx.record("unrelated:test:entry#refused-handles", root, || {
        for entry in [&foreign.entries()[0], &copied, &tampered] {
            let source_error = archive.entry_name_source(entry).expect_err("only a borrowed admitted handle has a source field");
            let read_error = archive.read_entry_name(&ctx, entry).expect_err("foreign handles cannot produce a semantic name");
            for error in [source_error, read_error] {
                assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message) if message == "ZIP entry does not belong to this archive snapshot"));
            }
        }
        Ok(())
    }).expect("commit refused handle control");
    let name = ctx
        .record("unrelated:test:entry#admitted-handle", root, || {
            archive.read_entry_name(&ctx, &archive.entries()[0])
        })
        .expect("commit admitted handle name read");
    assert_eq!(name, "original");
    let name_source = archive
        .entry_name_source(&archive.entries()[0])
        .expect("admitted filename source");
    let expected = u64_from_index(name_source.start())..u64_from_index(name_source.end());
    let captures = ctx.take_record_captures().expect("actual handle captures");
    assert_eq!(captures.len(), 2);
    let refused = captures
        .iter()
        .find(|capture| capture.id() == "unrelated:test:entry#refused-handles")
        .expect("refused handle capture");
    assert!(refused.reads().is_empty());
    let admitted = captures
        .iter()
        .find(|capture| capture.id() == "unrelated:test:entry#admitted-handle")
        .expect("admitted handle capture");
    assert_eq!(admitted.reads().len(), 1);
    assert_eq!(admitted.reads()["file"], [expected]);
}

#[test]
fn admitted_names_use_original_coordinates_for_nonzero_archive_view_starts() {
    for recovered in [false, true] {
        let bytes = fixture(&[0x82], 0, Vec::new(), recovered);
        let padding = b"outer-prefix";
        let mut wrapped = padding.to_vec();
        wrapped.extend_from_slice(&bytes);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&wrapped, &arena, &DecodePolicy::service())
                .expect("wrapped archive source session");
        let archive_source = root
            .child(padding.len(), wrapped.len())
            .expect("original nonzero archive view");
        let archive = ArchiveSnapshot::new(&ctx, archive_source).expect("admitted nonzero archive");
        let entry = &archive.entries()[0];
        assert_eq!(entry.is_recovered(), recovered);
        let raw = archive
            .entry_name_source(entry)
            .expect("original nonzero filename source");
        let field_start = if recovered {
            30
        } else {
            bytes
                .windows(4)
                .position(|window| window == b"PK\x01\x02")
                .expect("wrapped fixture central signature")
                + 46
        };
        let expected_start = padding.len() + field_start;
        assert_eq!(
            (raw.start(), raw.end()),
            (expected_start, expected_start + 1)
        );
        assert_eq!(raw.window(), &[0x82]);
        let name = ctx
            .record(
                "unrelated:test:entry#shifted-archive",
                archive_source,
                || archive.read_entry_name(&ctx, entry),
            )
            .expect("commit nonzero archive name read");
        assert_eq!(name, "é");
        let captures = ctx
            .take_record_captures()
            .expect("nonzero archive captures");
        assert_eq!(captures.len(), 1);
        assert_eq!(
            (captures[0].offset(), captures[0].byte_len()),
            (
                u64::try_from(padding.len()).expect("outer prefix length fits u64"),
                u64::try_from(bytes.len()).expect("archive length fits u64")
            )
        );
        let start = u64::try_from(expected_start).expect("physical filename start fits u64");
        assert_eq!(captures[0].reads().len(), 1);
        assert_eq!(captures[0].reads()["file"], [start..start + 1]);
    }
}
