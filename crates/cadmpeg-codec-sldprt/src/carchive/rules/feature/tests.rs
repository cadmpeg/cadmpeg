use super::{
    at_cut_list_tail, axistail, cut_list_sort_head, is_turn_block, scaled_bodies, solidfolder,
    turn_block_after_zeros, M12345,
};
use crate::carchive::reader::Reader;
use crate::carchive::reader::V;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::HashMap;

const LIST: &[u8] = b"\xff\xff\x01\x00\x08\x00suObList";

/// `zeros` zero bytes, the turn block with `reverse` at +2 and `word` at +5,
/// and a declared `suObList`.
fn revolve_mark(zeros: usize, reverse: u8, word: u32) -> Vec<u8> {
    let mut bytes = vec![0; zeros];
    bytes.extend([0, 0, reverse, 0, 0]);
    bytes.extend(word.to_le_bytes());
    bytes.extend(LIST);
    bytes.extend([0, 0]);
    bytes
}

fn with_reader<T>(bytes: &[u8], read: impl FnOnce(&Reader<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let reader = Reader::new(&ctx, bytes, 0, HashMap::new(), 0, None);
    read(&reader)
}

#[test]
fn a_turn_block_holds_a_reverse_byte_and_a_word() {
    for reverse in [0, 1] {
        for word in [0, 1] {
            let bytes = revolve_mark(0, reverse, word);
            assert_eq!(with_reader(&bytes, |r| is_turn_block(r, 0)), Ok(true));
        }
    }
    for (reverse, word) in [(2, 0), (0, 2)] {
        let bytes = revolve_mark(0, reverse, word);
        assert_eq!(with_reader(&bytes, |r| is_turn_block(r, 0)), Ok(false));
    }
    let mut led = revolve_mark(0, 1, 0);
    led[0] = 1;
    assert_eq!(with_reader(&led, |r| is_turn_block(r, 0)), Ok(false));
    let mut padded = revolve_mark(0, 1, 0);
    padded[3] = 1;
    assert_eq!(with_reader(&padded, |r| is_turn_block(r, 0)), Ok(false));
}

#[test]
fn zero_bytes_in_place_of_the_mark_record_end_at_the_turn_block() {
    for reverse in [0, 1] {
        for word in [0, 1] {
            let bytes = revolve_mark(64, reverse, word);
            assert_eq!(
                with_reader(&bytes, |r| turn_block_after_zeros(r, 0)),
                Ok(Some(64))
            );
        }
    }
    let short = revolve_mark(30, 1, 0);
    assert_eq!(
        with_reader(&short, |r| turn_block_after_zeros(r, 0)),
        Ok(None)
    );
}

/// A handle block's version `version`, twenty zero bytes, the seven f64 of an
/// axis tail (a value, a point and a direction) and its zero byte, with the
/// offset of the first f64.
fn axis_tail(version: u32) -> (Vec<u8>, usize) {
    let mut bytes = version.to_le_bytes().to_vec();
    bytes.extend([0; 20]);
    let start = bytes.len();
    for value in [0.005, 0.0375, 0.0025, 0.0, 0.0, -1.0, 0.0] {
        bytes.extend(f64::to_le_bytes(value));
    }
    bytes.push(0);
    bytes.extend([0xaa; 4]);
    (bytes, start)
}

#[test]
fn a_version_15000_axis_tail_holds_seven_f64_after_twenty_zero_bytes() {
    let (bytes, start) = axis_tail(15000);
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &bytes, start, HashMap::new(), 0, None);
    let V::List(values) = axistail(&mut reader).expect("the tail reads") else {
        panic!("seven f64");
    };
    assert_eq!(
        values,
        [0.005, 0.0375, 0.0025, 0.0, 0.0, -1.0, 0.0].map(V::from)
    );
    assert_eq!(reader.p, start + 57);
    let (other, start) = axis_tail(16000);
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&other, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &other, start, HashMap::new(), 0, None);
    assert!(axistail(&mut reader).is_err());
}

#[test]
fn a_full_handle_block_tail_after_a_version_and_zeros_is_read_whole() {
    let mut bytes = 15000u32.to_le_bytes().to_vec();
    bytes.extend([0; 20]);
    let start = bytes.len();
    bytes.extend([0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend(127u32.to_le_bytes());
    bytes.extend([0; 12]);
    bytes.extend(M12345);
    bytes.extend([0; 4]);
    bytes.extend(15000u32.to_le_bytes());
    bytes.extend([0; 20]);
    let tail = [0.005, 0.0375, 0.0025, 0.0, 0.0, -1.0, 0.0];
    for value in tail {
        bytes.extend(f64::to_le_bytes(value));
    }
    bytes.push(0);
    let end = bytes.len();
    bytes.extend([0xaa; 4]);
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, &bytes, start, HashMap::new(), 0, None);
    assert_eq!(
        axistail(&mut reader),
        Ok(V::List(tail.map(V::from).to_vec()))
    );
    assert_eq!(reader.p, end);
}

/// The scale tail read from the start of `bytes`, and the position after it.
fn read_scale_tail(bytes: &[u8]) -> (Result<(), crate::carchive::reader::Stop>, usize) {
    let arena = DecodeArena::new();
    let (ctx, _root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::desktop()).expect("context");
    let mut reader = Reader::new(&ctx, bytes, 0, HashMap::new(), 0, None);
    let result = scaled_bodies(&mut reader);
    (result, reader.p)
}

#[test]
fn a_scale_with_no_body_and_no_identifiers_ends_with_the_stream() {
    let mut bytes = 0u16.to_le_bytes().to_vec();
    bytes.extend(0u32.to_le_bytes());
    assert_eq!(read_scale_tail(&bytes), (Ok(()), bytes.len()));
}

#[test]
fn a_scale_with_no_body_before_another_object_ends_after_its_closing_bytes() {
    let mut bytes = 0u16.to_le_bytes().to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(100_001u32.to_le_bytes());
    bytes.extend([0xff; 4]);
    bytes.extend([0; 12]);
    let end = bytes.len();
    bytes.extend([0xaa; 2]);
    assert_eq!(read_scale_tail(&bytes), (Ok(()), end));
}

#[test]
fn a_scale_with_one_body_reads_its_pointer_and_closing_bytes() {
    let mut bytes = 1u16.to_le_bytes().to_vec();
    bytes.extend([0, 0]);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend([0xff; 4]);
    bytes.extend([0; 12]);
    let end = bytes.len();
    bytes.extend([0xaa; 2]);
    assert_eq!(read_scale_tail(&bytes), (Ok(()), end));
}

#[test]
fn a_scale_without_closing_bytes_before_more_stream_is_refused() {
    let mut bytes = 0u16.to_le_bytes().to_vec();
    bytes.extend(0u32.to_le_bytes());
    bytes.extend([0; 16]);
    bytes.extend([0xaa; 2]);
    assert!(read_scale_tail(&bytes).0.is_err());
}

#[test]
fn a_scale_with_part_of_its_closing_bytes_at_the_stream_end_is_refused() {
    let mut bytes = 0u16.to_le_bytes().to_vec();
    bytes.extend(0u32.to_le_bytes());
    bytes.extend([0xff, 0xff, 0xff, 0xff, 0, 0]);
    assert!(read_scale_tail(&bytes).0.is_err());
}

#[test]
fn a_scale_with_two_bodies_is_refused() {
    let mut bytes = 2u16.to_le_bytes().to_vec();
    bytes.extend([0; 16]);
    assert!(read_scale_tail(&bytes).0.is_err());
}

/// A cut-list folder tail whose word at +8 is `word`.
fn cut_list_tail(word: u32) -> Vec<u8> {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend([0xff; 4]);
    bytes.extend(word.to_le_bytes());
    bytes.extend([0; 23]);
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes
}

#[test]
fn a_cut_list_tail_holds_word_two_or_three() {
    for word in [2, 3] {
        assert!(
            with_reader(&cut_list_tail(word), at_cut_list_tail),
            "word {word}"
        );
    }
}

#[test]
fn a_cut_list_tail_with_another_word_is_not_a_tail() {
    for word in [0, 1, 4] {
        assert!(
            !with_reader(&cut_list_tail(word), at_cut_list_tail),
            "word {word}"
        );
    }
}

#[test]
fn a_cut_list_tail_cut_short_by_the_stream_end_is_not_a_tail() {
    let mut bytes = cut_list_tail(3);
    bytes.pop();
    assert!(!with_reader(&bytes, at_cut_list_tail));
}

const SORT_OPTIONS: &[u8] = b"\xff\xff\x01\x00\x16\x00moCutListSortOptions_c";
/// A cut-list sort head with `word` and `six` as its six bytes, then `next`.
fn sort_head(word: u32, six: [u8; 6], next: &[u8]) -> Vec<u8> {
    let mut bytes = word.to_le_bytes().to_vec();
    bytes.extend(six);
    bytes.extend(next);
    bytes
}

#[test]
fn a_cut_list_sort_head_holds_word_zero_or_one() {
    for word in [0, 1] {
        let bytes = sort_head(word, [0; 6], SORT_OPTIONS);
        assert_eq!(with_reader(&bytes, cut_list_sort_head), Some(word));
    }
}

#[test]
fn a_cut_list_sort_head_with_another_word_is_not_a_head() {
    let bytes = sort_head(2, [0; 6], SORT_OPTIONS);
    assert_eq!(with_reader(&bytes, cut_list_sort_head), None);
}

#[test]
fn a_cut_list_sort_head_with_a_nonzero_byte_is_not_a_head() {
    let bytes = sort_head(0, [0, 0, 0, 1, 0, 0], SORT_OPTIONS);
    assert_eq!(with_reader(&bytes, cut_list_sort_head), None);
}

#[test]
fn twelve_zero_bytes_are_not_a_cut_list_sort_head() {
    let bytes = sort_head(0, [0; 6], &[0, 0, 0x01, 0x80]);
    assert_eq!(with_reader(&bytes, cut_list_sort_head), None);
}

/// A `moSolidBodyFolder_c` feature with an empty block: an inline `moNodeName_c`
/// name, the named fields, the block head, twelve zero bytes, the 35 fixed bytes,
/// the empty +55 string and its 64-byte tail, the block u32 and the folder u32 0.
fn solid_body_folder_feature() -> Vec<u8> {
    let mut bytes = b"\xff\xff\x01\x00\x0c\x00moNodeName_c\xff\xfe\xff\x01A\x00".to_vec();
    bytes.extend([0; 4]);
    bytes.extend([0, 0, 0, 0]);
    bytes.extend(7u32.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend([0xff, 0xfe, 0xff, 0x00]);
    bytes.extend([0; 8]);
    bytes.extend([0; 12 + 35]);
    bytes.extend([0xff, 0xfe, 0xff, 0x00]);
    let mut tail = vec![0; 64];
    tail[..5].copy_from_slice(&[0x00, 0xff, 0xff, 0x00, 0x00]);
    tail[54..56].copy_from_slice(&[0x02, 0x00]);
    bytes.extend(tail);
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes
}

#[test]
fn a_solid_body_folder_reads_its_cut_list_sort_head_and_sort_options() {
    for word in [0, 1] {
        let mut bytes = solid_body_folder_feature();
        bytes.extend(sort_head(word, [0; 6], SORT_OPTIONS));
        bytes.extend([0; 10]);
        let arena = DecodeArena::new();
        let (ctx, _root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop())
            .expect("context");
        let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 0, None);
        let folder = solidfolder(&mut reader).expect("folder");
        assert_eq!(folder.get("sort_word").as_int(), Some(i64::from(word)));
        assert_eq!(reader.p, bytes.len());
    }
}

#[test]
fn a_cut_list_sort_head_cut_short_by_the_stream_end_is_not_a_head() {
    let bytes = sort_head(0, [0; 6], &[0xff]);
    assert_eq!(with_reader(&bytes, cut_list_sort_head), None);
}
