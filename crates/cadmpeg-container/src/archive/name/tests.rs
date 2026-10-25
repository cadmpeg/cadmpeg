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

fn expected_reads(
    declaration: u64,
    name: std::ops::Range<u64>,
    recovered: bool,
    extra_reads: &[(usize, usize)],
) -> Vec<std::ops::Range<u64>> {
    let mut ranges = if recovered {
        vec![
            declaration + 6..declaration + 8,
            declaration + 26..declaration + 28,
        ]
    } else {
        vec![declaration + 28..declaration + 32]
    };
    for (offset, len) in extra_reads {
        let start = name.end + u64::try_from(*offset).expect("extra field offset fits u64");
        ranges.push(start..start + u64::try_from(*len).expect("extra field length fits u64"));
    }
    ranges.push(name);
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<std::ops::Range<u64>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(previous) if range.start <= previous.end => {
                previous.end = previous.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}

fn check(bytes: &[u8], raw: &[u8], expected: &str, extra_reads: &[(usize, usize)]) {
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
    let reads = expected_reads(
        entry.declaration_range().start,
        start..end,
        entry.is_recovered(),
        extra_reads,
    );
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
            &[],
        );
        check(
            &fixture(&[0x82], flags, Vec::new(), false),
            &[0x82],
            "é",
            &[],
        );
    }
}

#[test]
fn central_unicode_path_uses_last_valid_original_checksum_and_exact_selected_fields() {
    let raw = b"original";
    let mut extra = unicode(raw, "first".as_bytes(), true);
    let before = extra.len();
    extra.extend(unicode(raw, "水".as_bytes(), true));
    let wrong = extra.len();
    extra.extend(unicode(raw, b"wrong", false));
    let invalid = extra.len();
    extra.extend(unicode(raw, &[0xff], true));
    check(
        &fixture(raw, 0, extra, false),
        raw,
        "水",
        &[
            (0, 4),
            (5, 4 + "first".len()),
            (before, 4),
            (before + 5, 4 + "水".len()),
            (wrong, 4),
            (wrong + 5, 4),
            (invalid, 4),
            (invalid + 5, 5),
        ],
    );
}

#[test]
fn invalid_unicode_paths_leave_original_name_and_read_only_original_field() {
    let raw = &[0x82];
    let mut extra = unicode(raw, b"wrong", false);
    let invalid = extra.len();
    extra.extend(unicode(raw, &[0xff], true));
    check(
        &fixture(raw, 0, extra, false),
        raw,
        "é",
        &[(0, 4), (5, 4), (invalid, 4), (invalid + 5, 5)],
    );
}

#[test]
fn local_recovery_keeps_flag_specific_lossy_utf8_and_ignores_unicode_extras() {
    check(
        &fixture(&[0xff], 0x800, Vec::new(), true),
        &[0xff],
        "�",
        &[],
    );
    check(
        &fixture("é".as_bytes(), 0, Vec::new(), true),
        "é".as_bytes(),
        "├⌐",
        &[],
    );
    let raw = &[0x82];
    check(
        &fixture(raw, 0, unicode(raw, "水".as_bytes(), true), true),
        raw,
        "é",
        &[],
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
    assert_eq!(
        admitted.reads()["file"],
        expected_reads(
            archive.entries()[0].declaration_range().start,
            expected,
            false,
            &[]
        )
    );
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
        assert_eq!(
            captures[0].reads()["file"],
            expected_reads(
                u64_from_index(padding.len()) + entry.declaration_range().start,
                start..start + 1,
                recovered,
                &[],
            )
        );
    }
}

#[test]
fn unknown_extra_headers_select_no_content_and_leave_their_body_unread() {
    let raw = b"original";
    let mut extra = vec![0x34, 0x12, 3, 0, 9, 8, 7];
    let unicode_at = extra.len();
    extra.extend(unicode(raw, "水".as_bytes(), true));
    check(
        &fixture(raw, 0, extra, false),
        raw,
        "水",
        &[(0, 4), (unicode_at, 4), (unicode_at + 5, 4 + "水".len())],
    );
}

#[test]
fn candidate_crc_changes_the_selected_name_and_its_genuine_content_reads() {
    let raw = b"original";
    for (valid_crc, expected) in [(false, "first"), (true, "second")] {
        let mut extra = unicode(raw, b"first", true);
        let second = extra.len();
        extra.extend(unicode(raw, b"second", valid_crc));
        let second_read = 4 + if valid_crc { "second".len() } else { 0 };
        check(
            &fixture(raw, 0, extra, false),
            raw,
            expected,
            &[
                (0, 4),
                (5, 4 + "first".len()),
                (second, 4),
                (second + 5, second_read),
            ],
        );
    }
}

#[test]
fn recovered_utf8_flag_changes_the_same_raw_name_inside_its_record() {
    for (flags, expected) in [(0, "\u{a0}"), (0x800, "�")] {
        check(
            &fixture(&[0xff], flags, Vec::new(), true),
            &[0xff],
            expected,
            &[],
        );
    }
}

#[test]
fn a_foreign_context_cannot_read_selectors_from_an_admitted_entry() {
    let bytes = fixture(b"original", 0, Vec::new(), false);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("original name session");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("original name archive");
    let foreign_arena = DecodeArena::new();
    let (foreign_ctx, foreign_root) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service())
            .expect("foreign name session");
    foreign_ctx
        .record("unrelated:test:entry#foreign-context", foreign_root, || {
            let error = archive
                .read_entry_name(&foreign_ctx, &archive.entries()[0])
                .expect_err("an admitted entry still belongs to its actual source session");
            assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message)
                if message == "record view belongs to another or detached decode session"));
            Ok(())
        })
        .expect("record the rejected foreign-context attempt");
    let captures = foreign_ctx
        .take_record_captures()
        .expect("foreign captures");
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
    assert!(ctx
        .take_record_captures()
        .expect("original captures")
        .is_empty());
}

#[test]
fn locating_the_original_filename_field_adds_no_semantic_read_credit() {
    let bytes = fixture(b"original", 0, Vec::new(), false);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("filename navigation session");
    let archive = ArchiveSnapshot::new(&ctx, root).expect("filename navigation archive");
    ctx.record("unrelated:test:entry#filename-navigation", root, || {
        let source = archive.entry_name_source(&archive.entries()[0])?;
        assert_eq!(source.window(), b"original");
        Ok(())
    })
    .expect("filename navigation does not parse a name");
    let captures = ctx.take_record_captures().expect("navigation captures");
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}
