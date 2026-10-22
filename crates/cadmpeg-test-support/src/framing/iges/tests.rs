// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use crate::framing::{FrameParent, FramedPart, PartTable};

use super::{frame, rebuild, Edit};

fn source(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("literal ASCII source");
    run(&ctx, root);
}

fn card(section: u8, sequence: u32, payload: &[u8], ending: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 72);
    let mut bytes = [b' '; 80].to_vec();
    bytes[..payload.len()].copy_from_slice(payload);
    bytes[72] = section;
    bytes[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    bytes.extend_from_slice(ending);
    bytes
}

fn directory(sequence: u32, ending: &[u8]) -> Vec<u8> {
    let payload = if sequence % 2 == 1 {
        b"     116       1       0       0       0       0       0       000000000"
    } else {
        b"     116       0       0       1      42       0       0LABEL          0"
    };
    card(b'D', sequence, payload, ending)
}

fn example() -> Vec<u8> {
    let mut bytes = card(b'S', 1, b"Start text", b"\n");
    bytes.extend(card(b'G', 1, b"1H,,1H;;", b"\r\n"));
    bytes.extend_from_slice(b"junk\r");
    bytes.extend(directory(1, b"\r"));
    bytes.extend(directory(2, b"\n"));
    bytes.extend(card(b'P', 1, b"116,1.,2.,3.;", b"\r\n"));
    bytes.extend(card(b'T', 1, b"S      1G      1D      2P      1", b"\n"));
    bytes.extend_from_slice(b"tail\n");
    bytes
}

fn selected<'a, 'b>(parts: &'b [FramedPart<'a>], level: &str) -> Vec<&'b FramedPart<'a>> {
    parts
        .iter()
        .filter(|part| part.level().as_str() == level)
        .collect()
}

#[test]
fn exact_mixed_ending_card_directory_framing_and_dispatch_come_from_source() {
    let bytes = example();
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("physical cards");
        PartTable::load()
            .expect("immutable table")
            .validate(ctx, &parts)
            .expect("exact parents and bounds");
        assert_eq!(parts.len(), 26);
        assert_eq!(parts[0].level().as_str(), "iges.start_section");
        assert_eq!(parts[0].framing(), [72..81]);
        assert_eq!(parts[0].dispatch(), [72..73]);
        assert_eq!(parts[1].parent(), FrameParent::Part(0));
        assert_eq!(parts[2].level().as_str(), "iges.unmarked_line");
        assert_eq!(parts[2].view().window(), b"junk\r");
        assert_eq!(parts[2].framing(), [167..168]);
        let entries = selected(&parts, "iges.directory_entry");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].view().start(), entries[0].view().end()),
            (168, 330)
        );
        assert_eq!(
            entries[0].framing(),
            [176..184, 240..249, 273..281, 321..330]
        );
        assert_eq!(entries[0].dispatch(), [168..176, 249..257, 281..289]);
        let fields = selected(&parts, "iges.directory_field");
        assert_eq!(fields.len(), 18);
        assert_eq!(fields[0].view().window(), b"     116");
        assert_eq!(fields[0].dispatch(), [168..176]);
        assert!(fields[0].framing().is_empty());
        assert_eq!(fields[1].framing(), [176..184]);
        assert_eq!(fields[12].framing(), [273..281]);
        assert_eq!(fields[13].dispatch(), [281..289]);
        let terminate = selected(&parts, "iges.terminate_card");
        assert_eq!(terminate[0].framing(), [412..444, 484..493]);
        assert_eq!(terminate[0].dispatch(), [484..485]);
        let tail = selected(&parts, "iges.post_terminate_line");
        assert_eq!(tail[0].view().window(), b"tail\n");
        assert_eq!(tail[0].framing(), [497..498]);
        assert!(ctx
            .take_record_captures()
            .expect("no semantic read captures")
            .is_empty());
    });
}

#[test]
fn packed_cards_require_all_markers_and_no_line_endings_follow_the_eighty_byte_grid() {
    for ending in [b"\r\n".as_slice(), b"".as_slice()] {
        let mut bytes = card(b'S', 1, b"Start", b"");
        bytes.extend(directory(1, b""));
        bytes.extend(directory(2, b""));
        bytes.extend(card(b'T', 1, b"S      1G      0D      2P      0", ending));
        source(&bytes, |ctx, view| {
            let mut parts = Vec::new();
            frame(ctx, view, &mut parts).expect("four actual cards");
            PartTable::load()
                .expect("table")
                .validate(ctx, &parts)
                .expect("packed containment");
            let entries = selected(&parts, "iges.directory_entry");
            assert_eq!(
                (entries[0].view().start(), entries[0].view().end()),
                (80, 240)
            );
            assert_eq!(entries[0].framing(), [88..96, 152..160, 184..192, 232..240]);
            assert_eq!(
                selected(&parts, "iges.terminate_card")[0].view().end(),
                bytes.len()
            );
        });
    }
    let mut malformed = card(b'S', 1, b"Start", b"");
    let mut false_card = directory(1, b"\n");
    false_card[72] = b'X';
    malformed.extend(false_card);
    malformed.extend(directory(3, b"\n"));
    source(&malformed, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("later genuine card remains locatable");
        assert!(selected(&parts, "iges.start_card").is_empty());
        let entries = selected(&parts, "iges.directory_entry");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].view().start(), 161);
        assert_eq!(selected(&parts, "iges.directory_field").len(), 9);
    });
}

#[test]
fn terminate_same_line_suffix_and_every_later_physical_line_are_tail_parts() {
    let mut bytes = card(b'T', 1, b"S      0G      0D      0P      0", b"");
    bytes.extend_from_slice(b"same-line\r\n");
    bytes.extend(card(
        b'S',
        1,
        b"This is tail, not a second Start section",
        b"\n",
    ));
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("actual Terminate marker");
        PartTable::load()
            .expect("table")
            .validate(ctx, &parts)
            .expect("tail containment");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].view().end(), 80);
        assert_eq!(parts[1].level().as_str(), "iges.post_terminate_line");
        assert_eq!(parts[1].view().window(), b"same-line\r\n");
        assert_eq!(parts[1].framing(), [89..91]);
        assert_eq!(parts[2].level().as_str(), "iges.post_terminate_line");
        assert_eq!(parts[2].view().start(), 91);
        assert!(selected(&parts, "iges.start_section").is_empty());
    });
}

#[test]
fn directory_positions_define_pairs_even_with_bad_sequences_and_cut_short_cards() {
    let mut bytes = directory(99, b"\n");
    bytes.extend(directory(2, b"\n"));
    let last = directory(55, b"");
    bytes.extend_from_slice(&last[..76]);
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("actual D markers and extents");
        PartTable::load()
            .expect("table")
            .validate(ctx, &parts)
            .expect("trailing singleton belongs to section");
        let entries = selected(&parts, "iges.directory_entry");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            (entries[0].view().start(), entries[0].view().end()),
            (0, 162)
        );
        assert_eq!(
            (entries[1].view().start(), entries[1].view().end()),
            (162, 238)
        );
        assert_eq!(entries[1].framing(), [170..178, 234..238]);
        assert_eq!(selected(&parts, "iges.directory_field").len(), 27);
    });
    let mut earlier_cut = card(b'S', 1, b"Start", b"\n");
    earlier_cut.extend_from_slice(&last[..71]);
    source(&earlier_cut, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("last line ends before its marker");
        assert!(selected(&parts, "iges.directory_entry").is_empty());
        let unmarked = selected(&parts, "iges.unmarked_line");
        assert_eq!(unmarked[0].view().window(), &last[..71]);
        assert!(unmarked[0].framing().is_empty());
    });
}

#[test]
fn equal_length_first_middle_last_directory_damage_changes_only_requested_bytes() {
    let bytes = example();
    source(&bytes, |ctx, view| {
        assert_eq!(rebuild(ctx, view, &[]).expect("no-op"), bytes);
        for range in [168..176, 232..240, 313..321] {
            for damage in [0_u8, 0xff] {
                let replacement = [damage; 8];
                let edited = rebuild(
                    ctx,
                    view,
                    &[Edit {
                        range: range.clone(),
                        replacement: &replacement,
                    }],
                )
                .expect("one actual Directory field");
                let mut expected = bytes.clone();
                expected[usize::try_from(range.start).expect("source offset")
                    ..usize::try_from(range.end).expect("source offset")]
                    .copy_from_slice(&replacement);
                assert_eq!(edited, expected);
                assert_eq!(edited.len(), bytes.len());
                assert_eq!(&edited[330..], &bytes[330..]);
            }
        }
    });
}

#[test]
fn rebuilder_protects_locator_sequence_census_line_endings_and_unimplemented_payloads() {
    let bytes = example();
    source(&bytes, |ctx, view| {
        for range in [
            176..184,
            273..281,
            240..248,
            412..420,
            80..81,
            330..331,
            81..82,
        ] {
            let replacement = vec![
                0xff;
                usize::try_from(range.end - range.start)
                    .expect("small literal interval")
            ];
            assert!(rebuild(
                ctx,
                view,
                &[Edit {
                    range,
                    replacement: &replacement
                }]
            )
            .is_err());
        }
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 168..176,
                replacement: b"short"
            }]
        )
        .is_err());
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 168..176,
                replacement: b"\n       "
            }]
        )
        .is_err());
        assert!(rebuild(
            ctx,
            view,
            &[
                Edit {
                    range: 168..176,
                    replacement: b"12345678"
                },
                Edit {
                    range: 172..176,
                    replacement: b"0000"
                }
            ]
        )
        .is_err());
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 184..200,
                replacement: b"1234567812345678"
            }]
        )
        .is_err());
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 999..1000,
                replacement: b"x"
            }]
        )
        .is_err());
    });
    let mut unmarked = card(b'S', 1, b"Start", b"\n");
    unmarked.extend(card(b'X', 1, b"Unmarked physical line", b"\n"));
    source(&unmarked, |ctx, view| {
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 153..154,
                replacement: b"D"
            }]
        )
        .is_err());
    });
}

#[test]
fn source_coordinates_are_absolute_and_foreign_or_retained_views_are_refused() {
    let mut bytes = b"prefix".to_vec();
    bytes.extend(directory(1, b"\n"));
    source(&bytes, |ctx, root| {
        let view = root.child(6, bytes.len()).expect("genuine source window");
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("retained actual address space");
        assert_eq!(parts[0].view().start(), 6);
        assert_eq!(
            selected(&parts, "iges.directory_field")[0].dispatch(),
            [6..14]
        );
        assert_eq!(
            rebuild(
                ctx,
                view,
                &[Edit {
                    range: 6..14,
                    replacement: b"     110"
                }]
            )
            .expect("absolute edit"),
            [&b"     110"[..], &bytes[14..]].concat()
        );
        assert!(frame(ctx, View::over_retained(&bytes), &mut Vec::new()).is_err());
        source(&bytes, |_, foreign| {
            assert!(frame(ctx, foreign, &mut Vec::new()).is_err());
        });
    });
}

#[test]
fn short_physical_lines_have_exact_unmarked_extents_without_invented_cards() {
    source(b"cut-short\r\nnext", |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("literal unmarked-line row");
        PartTable::load()
            .expect("table")
            .validate(ctx, &parts)
            .expect("physical line bounds");
        assert_eq!(parts.len(), 2);
        assert!(parts
            .iter()
            .all(|part| part.level().as_str() == "iges.unmarked_line"));
        assert_eq!(parts[0].view().window(), b"cut-short\r\n");
        assert_eq!(parts[0].framing(), [9..11]);
        assert_eq!(parts[1].view().window(), b"next");
        assert!(parts[1].framing().is_empty());
        assert_eq!(
            rebuild(ctx, view, &[]).expect("source preservation"),
            b"cut-short\r\nnext"
        );
    });
    source(b"", |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("empty source has no reached parts");
        assert!(parts.is_empty());
    });
}

#[test]
fn section_census_uses_actual_markers_and_budget_refusals_are_not_suppressed() {
    let mut bytes = card(b'S', 19, b"Start", b"\n");
    bytes.extend(directory(1, b"\n"));
    bytes.extend(card(b'T', 1, b"S    999G    999D    999P    999", b"\n"));
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("actual column census");
        assert_eq!(selected(&parts, "iges.start_card").len(), 1);
        assert_eq!(selected(&parts, "iges.directory_entry").len(), 1);
        assert_eq!(
            rebuild(ctx, view, &[]).expect("unchanged source counts"),
            bytes
        );
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("input independent of retained budget");
    assert!(frame(&ctx, view, &mut Vec::new()).is_err());
    assert!(
        matches!(ctx.resource_refusal(), Some(limit) if limit.scope == cadmpeg_core::decode::LimitScope::Session && limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}
