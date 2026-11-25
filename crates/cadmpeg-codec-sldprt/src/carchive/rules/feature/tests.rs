use super::{is_turn_block, turn_block_after_zeros};
use crate::carchive::reader::Reader;
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
