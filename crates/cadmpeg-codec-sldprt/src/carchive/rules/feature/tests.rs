use super::{axistail, is_turn_block, scaled_bodies, turn_block_after_zeros, M12345};
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
fn a_scale_with_two_bodies_stops_the_walk() {
    let mut bytes = 2u16.to_le_bytes().to_vec();
    bytes.extend([0; 16]);
    assert!(read_scale_tail(&bytes).0.is_err());
}
