use super::{turn_sense, TurnSense};

const DECLARED_LIST: &[u8] = b"\xff\xff\x01\x00\x08\x00suObList";
const LIST_TOKEN: &[u8] = &[0x52, 0x80];
const AXIS_TOKEN: &[u8] = &[0x53, 0x80];

/// An object interval: zero bytes, the turn block with `reverse` at +2 and
/// `word` at +5, the list reference `list`, an empty list, the u32 `form`, a
/// u32 and the axis reference.
fn object(reverse: u8, word: u32, list: &[u8], form: u32) -> Vec<u8> {
    let mut payload = vec![0; 40];
    payload.extend([0, 0, reverse, 0, 0]);
    payload.extend(word.to_le_bytes());
    payload.extend(list);
    payload.extend(0u16.to_le_bytes());
    payload.extend(form.to_le_bytes());
    payload.extend(1u32.to_le_bytes());
    payload.extend(AXIS_TOKEN);
    payload.extend([0; 16]);
    payload
}

fn read(payload: &[u8]) -> Option<TurnSense> {
    turn_sense(payload, 0, payload.len())
}

#[test]
fn the_reverse_byte_states_the_turn_sense() {
    for list in [LIST_TOKEN, DECLARED_LIST] {
        for word in [0, 1] {
            for form in [3, 5] {
                assert_eq!(read(&object(0, word, list, form)), Some(TurnSense::Stored));
                assert_eq!(
                    read(&object(1, word, list, form)),
                    Some(TurnSense::Reversed)
                );
            }
        }
    }
}

#[test]
fn a_block_of_another_form_states_no_sense() {
    assert_eq!(read(&object(2, 0, LIST_TOKEN, 3)), None);
    assert_eq!(read(&object(0, 2, LIST_TOKEN, 3)), None);
    assert_eq!(read(&object(0, 0, LIST_TOKEN, 4)), None);
    let mut led = object(1, 0, LIST_TOKEN, 3);
    led[41] = 1;
    assert_eq!(read(&led), None);
    let mut renamed = object(1, 0, DECLARED_LIST, 3);
    let name = 40 + 9 + 6;
    assert_eq!(&renamed[name..name + 2], b"su");
    renamed[name + 7] = b'x';
    assert_eq!(read(&renamed), None);
    let mut padded = object(1, 0, LIST_TOKEN, 3);
    padded[43] = 1;
    assert_eq!(read(&padded), None);
    let mut listed = object(1, 0, LIST_TOKEN, 3);
    listed[51] = 1;
    assert_eq!(read(&listed), None);
    let mut unreferenced = object(1, 0, LIST_TOKEN, 3);
    unreferenced[62] = 0;
    assert_eq!(read(&unreferenced), None);
}

#[test]
fn two_turn_blocks_state_no_sense() {
    let mut payload = object(0, 0, LIST_TOKEN, 3);
    payload.extend(object(0, 0, LIST_TOKEN, 3));
    assert_eq!(read(&payload), None);
}

#[test]
fn the_block_lies_inside_the_object_interval() {
    let payload = object(1, 0, LIST_TOKEN, 3);
    assert_eq!(turn_sense(&payload, 0, 50), None);
    assert_eq!(turn_sense(&payload, 41, payload.len()), None);
    assert_eq!(turn_sense(&payload, 0, payload.len() + 1), None);
}
