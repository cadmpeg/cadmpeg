// SPDX-License-Identifier: Apache-2.0
//! Walker contracts over synthetic streams.

use cadmpeg_core::decode::View;

use crate::dialect::Grammar;

use super::SlotKind::{Other, Shell, Text, Texture};
use super::{Record, Walker, MAX_SEGMENT_DEPTH};
use crate::test_support::hsf::{face_tag, range_block, translation, Stream};

/// Walks `bytes` to the end record and returns the record count.
fn walk(bytes: &[u8]) -> Result<usize, super::WalkError> {
    walk_as(Grammar::Resaved, bytes)
}

/// Walks `bytes` under the rules of `grammar`.
fn walk_as(grammar: Grammar, bytes: &[u8]) -> Result<usize, super::WalkError> {
    let mut walker = Walker::new(View::over_retained(bytes), grammar);
    let mut count = 0;
    loop {
        count += 1;
        if matches!(walker.next()?, Record::End) {
            return Ok(count);
        }
    }
}

/// Every record kind once, balanced, ending with the trailer and `z`.
fn every_record() -> Vec<u8> {
    every_record_of(Grammar::Resaved)
}

/// Every record kind of `grammar` once, balanced, ending with the trailer and
/// `z`. The `SolidWorks` form adds the records that only it holds.
fn every_record_of(grammar: Grammar) -> Vec<u8> {
    let mut s = Stream::of(grammar);
    s.header(3).camera("default").user_option("length=`3`");
    s.line_styles()
        .open("a")
        .style("/style")
        .conditional_style("/s", "isvisible");
    s.condition("disp0")
        .tag0()
        .eq_record()
        .h_attribute()
        .r_attribute();
    s.modelling_matrix(&translation([1.0, 2.0, 3.0]))
        .texture_matrix();
    s.colour([1, 2, 3])
        .line_pattern("edw-dashed")
        .edge_pattern("edw-dashed")
        .marker(9)
        .wide_user_option("name=`\u{e9}`")
        .material("diffuse")
        .include("/include library/x");
    s.v_text().slot().v_texture("t").slot();
    s.shell_placeholder()
        .slot()
        .shell(0, &[[0.0; 3]; 3], &[vec![0, 1, 2]]);
    if grammar == Grammar::SolidWorks {
        s.solidworks_material_extra([1, 2, 3, 0, 4, 5, 6])
            .solidworks_h_name()
            .solidworks_r_run(0)
            .solidworks_r_run(36);
    }
    s.shell_with_tail(
        2,
        &[[0.0; 3]; 3],
        &[vec![0, 1, 2]],
        &[range_block(3, [0.0, 0.5]), face_tag("swfaceid=7", false)].concat(),
    );
    s.instance(0, 1, &translation([0.0; 3])).pad();
    s.close().separator();
    s.side_text(0, "ab").close();
    s.side_empty(1).close();
    s.side_geometry(3, &geometry_items()).close();
    s.side_image(2, b"jpeg").close();
    s.separator().trailer_and_end();
    s.into_bytes()
}

/// One of each geometry item: point, line, arc, circle, and a polyline set of
/// two polylines.
fn geometry_items() -> Vec<u8> {
    let mut items = Vec::new();
    let floats = |count: usize| vec![0x3f_u8; 4 * count];
    items.push(b'X');
    items.extend(floats(3));
    items.push(b'l');
    items.extend(floats(6));
    for code in *b"Cc" {
        items.push(code);
        items.extend(floats(9));
        items.push(0);
    }
    items.extend([0x10, 0x00, 0x0c]);
    items.extend(3_u32.to_le_bytes());
    items.extend(1_u32.to_le_bytes());
    items.extend(2_u32.to_le_bytes());
    items.extend(floats(2 * 3 + 2));
    items
}

impl Stream {
    fn pad(&mut self) -> &mut Self {
        self.raw(&[0])
    }
}

#[test]
fn every_opcode_walks_to_the_end_record() {
    assert!(walk(&every_record()).is_ok());
}

#[test]
fn record_kinds_and_slot_kinds_are_reported() {
    let mut s = Stream::new();
    s.v_text()
        .slot()
        .v_texture("t")
        .slot()
        .shell_placeholder()
        .slot()
        .slot();
    s.colour([9, 8, 7]).slot().raw(b"z");
    let bytes = s.into_bytes();
    let mut walker = Walker::new(View::over_retained(&bytes), Grammar::Resaved);
    let mut slots = Vec::new();
    loop {
        match walker.next().unwrap() {
            Record::Slot(kind) => slots.push(kind),
            Record::End => break,
            Record::Colour(rgb) => assert_eq!(rgb, [9, 8, 7]),
            _ => {}
        }
    }
    assert_eq!(slots, [Text, Texture, Shell, Shell, Other]);
}

#[test]
fn every_proper_prefix_is_an_error_and_never_a_panic() {
    let bytes = every_record();
    for len in 0..bytes.len() {
        let cut = bytes.get(..len).unwrap();
        assert!(walk(cut).is_err(), "prefix of {len} bytes walked");
    }
}

#[test]
fn an_unknown_opcode_names_its_offset() {
    let mut s = Stream::new();
    s.open("a").close().raw(&[0x99]);
    let error = walk(s.bytes()).unwrap_err();
    assert_eq!((error.offset, error.reason), (4, "unknown opcode"));
}

#[test]
fn segments_must_balance() {
    let mut s = Stream::new();
    s.close().raw(b"z");
    assert_eq!(
        walk(s.bytes()).unwrap_err().reason,
        "unbalanced segment close"
    );
    let mut s = Stream::new();
    s.open("a").raw(b"z");
    assert_eq!(
        walk(s.bytes()).unwrap_err().reason,
        "end record inside an open segment"
    );
    assert!(walk(b"z\0").is_err());
    assert!(walk(b"z").is_ok());
}

#[test]
fn segment_depth_is_capped() {
    let mut s = Stream::new();
    for _ in 0..=MAX_SEGMENT_DEPTH {
        s.open("a");
    }
    assert_eq!(
        walk(s.bytes()).unwrap_err().reason,
        "segment nesting exceeds the cap"
    );
    let mut s = Stream::new();
    for _ in 0..MAX_SEGMENT_DEPTH {
        s.open("a");
    }
    for _ in 0..MAX_SEGMENT_DEPTH {
        s.close();
    }
    s.raw(b"z");
    assert!(walk(s.bytes()).is_ok());
}

#[test]
fn shell_counts_are_checked_against_the_stream() {
    let huge = |count: u32| {
        let mut bytes = vec![b'S', 0x0e, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes
    };
    assert_eq!(
        walk(&huge(0)).unwrap_err().reason,
        "shell: vertex count is zero or exceeds the cap"
    );
    assert_eq!(
        walk(&huge(u32::MAX)).unwrap_err().reason,
        "shell: vertex count is zero or exceeds the cap"
    );
    assert_eq!(
        walk(&huge(1_000_000)).unwrap_err().reason,
        "shell: vertices exceed the stream"
    );
}

#[test]
fn attribute_masks_must_be_known_and_bounded() {
    assert_eq!(
        walk(&[b'H', 0x07, 0x00, 0x00, 0x00]).unwrap_err().reason,
        "attribute mask is not in the known table"
    );
    let endless = [
        b'H', 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    assert_eq!(
        walk(&endless).unwrap_err().reason,
        "attribute mask has too many words"
    );
    assert!(walk(&[b'V', 0, 0]).is_err());
}

#[test]
fn bracket_records_follow_their_rules() {
    // A header must close with a bracket at its declared length.
    assert!(walk(&[b'[', 2, 0, 0, 0, 1, 1, 1, b'x', b'z']).is_err());
    assert!(walk(&[b'[', 2, 0, 0, 0, 1, 1, 1, b']', b'z']).is_ok());
    // A bracket record past offset 0 must be the 24 byte trailer.
    assert!(walk(b"z[").is_err());
    let mut s = Stream::new();
    s.raw(&[1]).raw(&[b'[', 0, 0, 0, 0]).raw(b"z");
    assert!(walk(s.bytes()).is_err());
}

#[test]
fn text_objects_accept_the_long_length_form() {
    let text = "x".repeat(200);
    let mut s = Stream::new();
    s.side_text(5, &text).close().raw(b"z");
    let bytes = s.into_bytes();
    let mut walker = Walker::new(View::over_retained(&bytes), Grammar::Resaved);
    match walker.next().unwrap() {
        Record::SideText(5, utf16) => assert_eq!(utf16.len(), 400),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_solidworks_record_walks_to_the_end_record() {
    let bytes = every_record_of(Grammar::SolidWorks);
    assert!(walk_as(Grammar::SolidWorks, &bytes).is_ok());
    // The same bytes break a rule of the re-saved grammar.
    assert!(walk_as(Grammar::Resaved, &bytes).is_err());
}

#[test]
fn every_proper_prefix_of_a_solidworks_stream_is_an_error() {
    let bytes = every_record_of(Grammar::SolidWorks);
    for len in 0..bytes.len() {
        let cut = bytes.get(..len).unwrap();
        assert!(
            walk_as(Grammar::SolidWorks, cut).is_err(),
            "prefix of {len} bytes walked"
        );
    }
}

#[test]
fn the_solidworks_header_closes_one_byte_later() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.header(3).raw(b"z");
    assert_eq!(s.bytes().get(..5), Some(&[b'[', 3, 0, 0, 0][..]));
    assert_eq!(s.bytes().get(10), Some(&b']'));
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok());
    assert!(walk_as(Grammar::Resaved, s.bytes()).is_err());
    let mut resaved = Stream::new();
    resaved.header(3).raw(b"z");
    let error = walk_as(Grammar::SolidWorks, resaved.bytes()).unwrap_err();
    assert_eq!(
        (error.offset, error.reason),
        (0, "header block: no closing bracket")
    );
}

#[test]
fn solidworks_v_templates_replace_the_re_saved_ones() {
    let text = [b'V', 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x03, 0x00, b'z'];
    assert!(walk_as(Grammar::SolidWorks, &text).is_ok());
    assert_eq!(
        walk_as(Grammar::Resaved, &text).unwrap_err().reason,
        "unknown V payload"
    );
    let old_text = [b'V', 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x07, 0x00, b'z'];
    let error = walk_as(Grammar::SolidWorks, &old_text).unwrap_err();
    assert_eq!((error.offset, error.reason), (0, "unknown V payload"));
    let sheet = [b'V', 0x80, 0x00, 0x00, 0x01, 0x00, 0x00, b'z'];
    assert_eq!(
        walk_as(Grammar::SolidWorks, &sheet).unwrap_err().reason,
        "unknown V payload"
    );
    let two_d = [b'V', 0xdf, 0x56, 0x0e, 0x20, 0x00, 0x00, b'z'];
    assert!(walk_as(Grammar::SolidWorks, &two_d).is_ok());
}

#[test]
fn a_solidworks_texture_is_a_record_then_a_short_v() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.v_texture("thread-mod").slot().raw(b"z");
    let bytes = s.into_bytes();
    let mut walker = Walker::new(View::over_retained(&bytes), Grammar::SolidWorks);
    let mut slots = Vec::new();
    loop {
        match walker.next().unwrap() {
            Record::Slot(kind) => slots.push(kind),
            Record::End => break,
            _ => {}
        }
    }
    assert_eq!(slots, [Texture]);
    // The record `t` is not an opcode of the re-saved grammar.
    let error = walk(&bytes).unwrap_err();
    assert_eq!((error.offset, error.reason), (0, "unknown opcode"));
}

#[test]
fn a_solidworks_material_with_bit_three_carries_seven_more_bytes() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.solidworks_material_extra([0xff, 0xff, 0xff, 0, 0x7f, 0x7f, 0x7f])
        .raw(b"z");
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok());
    // One byte short.
    let mut short = Stream::of(Grammar::SolidWorks);
    short.raw(&[b'"', 1, 9, 0, 1, 2, 3, 0, 4, 5]);
    assert_eq!(
        walk_as(Grammar::SolidWorks, short.bytes())
            .unwrap_err()
            .reason,
        "record is truncated"
    );
    let mut plain = Stream::of(Grammar::SolidWorks);
    plain
        .solidworks_material([0.294_118, 0.294_118, 0.294_118])
        .raw(b"z");
    assert!(walk_as(Grammar::SolidWorks, plain.bytes()).is_ok());
}

#[test]
fn the_material_text_is_reported() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.solidworks_material([0.5, 0.25, 1.0]).raw(b"z");
    let bytes = s.into_bytes();
    let mut walker = Walker::new(View::over_retained(&bytes), Grammar::SolidWorks);
    match walker.next().unwrap() {
        Record::Material(text) => assert_eq!(
            text,
            b"diffuse color=(r=0.500000 g=0.250000 b=1.000000)".as_slice()
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_solidworks_run_record_is_a_head_and_default_pairs() {
    for pairs in [0, 1, 36] {
        let mut s = Stream::of(Grammar::SolidWorks);
        s.solidworks_r_run(pairs).raw(b"z");
        assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok(), "{pairs}");
    }
    // A truncated head.
    let mut cut = Stream::of(Grammar::SolidWorks);
    cut.solidworks_r_run(0);
    let short = cut.bytes().get(..cut.bytes().len() - 1).unwrap().to_vec();
    assert_eq!(
        walk_as(Grammar::SolidWorks, &short).unwrap_err().reason,
        "record is truncated"
    );
}

#[test]
fn an_unknown_solidworks_mask_fails_with_its_offset() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.open("a")
        .raw(&[b'R', 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00])
        .raw(b"z");
    let error = walk_as(Grammar::SolidWorks, s.bytes()).unwrap_err();
    assert_eq!(
        (error.offset, error.reason),
        (3, "attribute mask is not in the known table")
    );
    let mut s = Stream::of(Grammar::SolidWorks);
    s.raw(&[b'H', 0x08, 0x00, 0x08, 0x00]).raw(b"z");
    let error = walk_as(Grammar::SolidWorks, s.bytes()).unwrap_err();
    assert_eq!(
        (error.offset, error.reason),
        (0, "attribute mask is not in the known table")
    );
}

#[test]
fn the_h_masks_of_solidworks_streams_have_their_extras() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.h_attribute().solidworks_h_name().raw(b"z");
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok());
    assert!(walk_as(Grammar::Resaved, s.bytes()).is_ok());
}

#[test]
fn solidworks_line_styles_need_no_table_opcode() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.line_styles().raw(b"z");
    assert_eq!(s.bytes().first(), Some(&0x4a));
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok());
    let error = walk_as(Grammar::Resaved, s.bytes()).unwrap_err();
    assert_eq!((error.offset, error.reason), (0, "unknown opcode"));
    // An empty table still opens with `h 01`.
    assert!(walk_as(Grammar::SolidWorks, &[b'h', 1, b'z']).is_ok());
}

#[test]
fn a_solidworks_image_has_a_one_byte_header() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.side_image(2, b"jpeg").close().raw(b"z");
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok());
    assert!(walk_as(Grammar::Resaved, s.bytes()).is_err());
}

#[test]
fn the_trailer_code_of_solidworks_streams_is_not_a_rule() {
    let mut s = Stream::of(Grammar::SolidWorks);
    s.separator().trailer_and_end();
    assert_eq!(s.bytes().get(6..8), Some(&[0xbe, 0x2c][..]));
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_ok());
}

/// The kinds of the records of `bytes`, up to the end record.
fn kinds(grammar: Grammar, bytes: &[u8]) -> Vec<String> {
    let mut walker = Walker::new(View::over_retained(bytes), grammar);
    let mut kinds = Vec::new();
    loop {
        let record = walker.next().unwrap();
        let end = matches!(record, Record::End);
        kinds.push(
            format!("{record:?}")
                .split(['(', ' '])
                .next()
                .unwrap()
                .to_owned(),
        );
        if end {
            return kinds;
        }
    }
}

#[test]
fn the_weight_pattern_and_colour_chain_is_three_records() {
    let mut s = Stream::new();
    s.eq_record().raw(b"z");
    assert_eq!(
        kinds(Grammar::Resaved, s.bytes()),
        ["Ignored", "Ignored", "Colour", "End"]
    );
    let mut s = Stream::new();
    s.line_pattern("edw-chain")
        .edge_pattern("edw-chain")
        .marker(7)
        .raw(b"z");
    assert_eq!(
        kinds(Grammar::Resaved, s.bytes()),
        ["Ignored", "Ignored", "Ignored", "End"]
    );
}

#[test]
fn a_pattern_flag_other_than_ff_is_an_error() {
    for opcode in [b'-', 0x0d] {
        let error = walk(&[opcode, 0x00, 0x00, b'z']).unwrap_err();
        assert_eq!(
            (error.offset, error.reason),
            (0, "line pattern: flag is not ff")
        );
    }
}

#[test]
fn a_colour_reads_mask_bytes_while_the_top_bit_is_set() {
    let colour = |bytes: &[u8]| {
        let mut data = bytes.to_vec();
        data.push(b'z');
        let mut walker = Walker::new(View::over_retained(&data), Grammar::Resaved);
        let record = walker.next().unwrap();
        assert!(matches!(walker.next(), Ok(Record::End)));
        match record {
            Record::Colour(rgb) => rgb,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(colour(&[b'~', 0x04, 1, 2, 3]), [1, 2, 3]);
    assert_eq!(colour(&[b'~', 0x9f, 0x70, 4, 5, 6]), [4, 5, 6]);
    let endless = walk(&[b'~', 0xff, 0xff, 0xff, 0xff, 0xff, 1, 2, 3, b'z']);
    assert_eq!(
        endless.unwrap_err().reason,
        "colour mask has too many bytes"
    );
}

#[test]
fn a_wide_user_option_counts_characters() {
    let mut s = Stream::new();
    s.wide_user_option("name=`\u{e9}\u{65e5}`").raw(b"z");
    assert_eq!(s.bytes().get(1..3), Some(&[9, 0][..]));
    let bytes = s.into_bytes();
    let mut walker = Walker::new(View::over_retained(&bytes), Grammar::Resaved);
    match walker.next().unwrap() {
        Record::UserOptionWide(utf16) => assert_eq!(utf16.len(), 18),
        other => panic!("{other:?}"),
    }
    // The declared count overruns the stream.
    assert_eq!(
        walk(&[0x16, 2, 0, b'a', 0, b'z']).unwrap_err().reason,
        "record is truncated"
    );
}

#[test]
fn a_shell_range_block_and_face_tag_end_before_the_terminator() {
    let points = [[0.0; 3]; 3];
    let strips = [vec![0, 1, 2]];
    let shell = |tail: &[u8]| {
        let mut s = Stream::new();
        s.shell_with_tail(0, &points, &strips, tail).raw(b"z");
        s.into_bytes()
    };
    let range = range_block(3, [0.25, 0.75]);
    for tail in [
        range.clone(),
        face_tag("swfaceid=12", false),
        face_tag("swfaceid=\u{e9}", true),
        [range.clone(), face_tag("swfaceid=3", false)].concat(),
    ] {
        assert!(walk(&shell(&tail)).is_ok());
    }
    let reason = |tail: &[u8]| walk(&shell(tail)).unwrap_err().reason;
    // The face tag comes after the range block.
    let swapped = [face_tag("swfaceid=3", false), range.clone()].concat();
    assert_eq!(reason(&swapped), "shell: no terminator");
    assert_eq!(
        reason(&range_block(2, [0.0, 1.0])),
        "shell: range block length is not twice the vertex count"
    );
    let mut bad_head = range.clone();
    bad_head[2] = 3;
    assert_eq!(reason(&bad_head), "shell: bad range block head");
    let mut bad_u = range.clone();
    bad_u[3..7].copy_from_slice(&1.0_f32.to_le_bytes());
    assert_eq!(reason(&bad_u), "shell: range block u is not zero");
    let mut bad_mark = range.clone();
    bad_mark[19] = 9;
    assert_eq!(reason(&bad_mark), "shell: no range block marker");
    assert_eq!(reason(&[0x63, 0x41]), "shell: face tag without user option");
    let mut unterminated = face_tag("a", false);
    unterminated.pop();
    unterminated.push(7);
    assert_eq!(
        reason(&unterminated),
        "shell: no zero byte after the face tag"
    );
}

#[test]
fn a_geometry_side_object_is_a_scope_of_items() {
    let mut s = Stream::new();
    s.side_geometry(4, &geometry_items()).close().raw(b"z");
    assert_eq!(
        kinds(Grammar::Resaved, s.bytes()),
        ["SideGeometry", "Close", "End"]
    );
    // Every proper prefix of the items is an error.
    let items = geometry_items();
    for len in 1..items.len() {
        let mut s = Stream::new();
        s.side_geometry(4, items.get(..len).unwrap());
        assert!(walk(s.bytes()).is_err(), "prefix of {len} bytes walked");
    }
}

#[test]
fn geometry_items_are_checked() {
    let side = |items: &[u8]| {
        let mut s = Stream::new();
        s.side_geometry(0, items).close().raw(b"z");
        walk(s.bytes())
    };
    let mut arc = vec![b'C'];
    arc.extend([0; 36]);
    arc.push(1);
    assert_eq!(
        side(&arc).unwrap_err().reason,
        "geometry object: no zero byte after the arc"
    );
    // The lengths pass the total.
    let mut overrun = vec![0x10, 0, 0x0c];
    overrun.extend(3_u32.to_le_bytes());
    overrun.extend(2_u32.to_le_bytes());
    overrun.extend(2_u32.to_le_bytes());
    assert_eq!(
        side(&overrun).unwrap_err().reason,
        "polyline set: lengths overrun the total"
    );
    // A huge total reads lengths up to the end of the stream.
    let mut huge = vec![0x10, 0, 0x0c];
    huge.extend(u32::MAX.to_le_bytes());
    huge.extend(1_u32.to_le_bytes());
    assert_eq!(side(&huge).unwrap_err().reason, "record is truncated");
    // One length covers the total but the arrays do not fit.
    let mut short = vec![0x10, 0, 0x0c];
    short.extend(u32::MAX.to_le_bytes());
    short.extend(u32::MAX.to_le_bytes());
    assert_eq!(side(&short).unwrap_err().reason, "record is truncated");
    // A byte that is no item makes the object empty.
    assert_eq!(
        kinds(Grammar::Resaved, &[b's', 0, 0, 0, 0, b')', b'z']),
        ["SideEmpty", "Close", "End"]
    );
}

#[test]
fn the_late_solidworks_grammar_mixes_the_layouts() {
    let mut s = Stream::of(Grammar::SolidWorksLate);
    s.header(3)
        .camera("default")
        .line_styles()
        .v_text()
        .slot()
        .v_texture("thread-mod")
        .slot();
    s.side_image(2, b"jpeg").close();
    s.separator().trailer_and_end();
    assert!(walk_as(Grammar::SolidWorksLate, s.bytes()).is_ok());
    // The image header is the u32 form of the re-saved grammar.
    assert!(walk_as(Grammar::SolidWorks, s.bytes()).is_err());
    assert!(walk_as(Grammar::Resaved, s.bytes()).is_err());
    let mut resaved_image = Stream::new();
    resaved_image.side_image(2, b"jpeg").close().raw(b"z");
    assert!(walk_as(Grammar::SolidWorksLate, resaved_image.bytes()).is_ok());
}

#[test]
fn the_run_and_extra_forms_hold_in_every_grammar() {
    // The mask `0x00100000` with default bytes `ff` instead of pairs.
    let mut s = Stream::new();
    s.solidworks_r_run(0).raw(&[0xff, 0xff, 0xff]).raw(b"z");
    for grammar in [
        Grammar::Resaved,
        Grammar::SolidWorks,
        Grammar::SolidWorksLate,
    ] {
        assert!(walk_as(grammar, s.bytes()).is_ok());
    }
    // The mask list `0x80010000 0x80000000 0x00000002` with its extra byte.
    let mut list = vec![b'R'];
    for word in [0x8001_0000_u32, 0x8000_0000, 0x0000_0002] {
        list.extend(word.to_le_bytes());
    }
    list.extend([0; 12]);
    list.extend([1, b'z']);
    assert!(walk(&list).is_ok());
    // A material with bit 3 holds seven more bytes in the re-saved grammar.
    let mut s = Stream::new();
    s.solidworks_material_extra([0xff, 0xff, 0xff, 0, 0x7f, 0x7f, 0x7f])
        .raw(b"z");
    assert!(walk(s.bytes()).is_ok());
    // The `H` mask `0x0004` is no longer limited to one grammar.
    let mut s = Stream::new();
    s.raw(&[b'H', 0x04, 0x00, 0x04, 0x00, 0x01, b'z']);
    assert!(walk(s.bytes()).is_ok());
}
