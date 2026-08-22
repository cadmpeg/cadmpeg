use super::*;

#[test]
fn operation_state_indices_retain_each_admitted_form() {
    let bytes = [
        0x7f, 0x83, 0xf9, 0x90, 0x12, 0x34, 0xa3, 0x1f, 0x85, 0xf1, 0x04, 0x2d, 0xff,
    ];
    let expected = [
        (Some(0x7f), OperationStateIndexForm::Direct, 1),
        (Some(0x3f9), OperationStateIndexForm::Compact, 2),
        (Some(0x1234), OperationStateIndexForm::Wide16, 3),
        (Some(0x31f85), OperationStateIndexForm::Wide20, 3),
        (Some(0x42d), OperationStateIndexForm::Extended16, 3),
        (None, OperationStateIndexForm::Null, 1),
    ];

    let mut at = 0;
    for (value, form, width) in expected {
        let token = super::operation_state_index(&bytes, at).expect("complete state index");
        assert_eq!(token.value, value);
        assert_eq!(token.form, form);
        assert_eq!(token.raw, &bytes[at..at + width]);
        assert_eq!(token.offset, at);
        at += width;
    }
    assert_eq!(at, bytes.len());
}

#[test]
fn operation_state_tagged_values_retain_width_and_value() {
    let cases = [
        (
            [0xaa, 0x60, 0x6b, 0, 0],
            OperationStateTaggedValueForm::Two,
            0x000a_606b,
            3,
        ),
        (
            [0xc0, 0x1a, 0x3f, 0x40, 0],
            OperationStateTaggedValueForm::Three,
            0x001a_3f40,
            4,
        ),
        (
            [0xe0, 0x01, 0x02, 0x03, 0x04],
            OperationStateTaggedValueForm::Four,
            0x0102_0304,
            5,
        ),
        (
            [0xff, 0x80, 0x00, 0x00, 0x01],
            OperationStateTaggedValueForm::Four,
            0x8000_0001,
            5,
        ),
    ];

    for (raw, form, value, width) in cases {
        let token = super::operation_state_tagged_value(&raw, 0).expect("complete tagged value");
        assert_eq!(token.form, form);
        assert_eq!(token.value, value);
        assert_eq!(token.marker, raw[0]);
        assert_eq!(token.raw, &raw[..width]);
        assert_eq!(token.offset, 0);
    }
}

#[test]
fn operation_state_counter_map_anchors_to_the_longest_bounded_suffix() {
    let mut bytes = vec![0x41, 0x83, 0x20, 0x3f];
    bytes.extend([
        0x05, 0x01, 0x90, 0x12, 0x34, 0x56, 0x57, 0x4e, 0x05, 0x02, 0xa3, 0x1f, 0x85, 0x2a, 0x2b,
        0x4e, 0x05, 0x01, 0x7d, 0x63, 0x63, 0x4e,
    ]);
    bytes.extend([
        0xb8, 0x6e, 0x58, 0x81, 0xd8, 0xb9, 0x96, 0x62, 0xdf, 0x59, 0xb8, 0x59, 0xc0, 0xd1, 0xf1,
        0xed,
    ]);

    let map = super::operation_state_counter_map(&bytes, 1000).expect("counter-map suffix");
    assert_eq!(map.offset, 1004);
    assert_eq!(map.rows.len(), 3);
    assert_eq!(map.end_offset, 1004 + 8 + 8 + 6);
    assert_eq!(map.trailing_bytes.len(), 16);
    assert_eq!(map.rows[0].row_kind, 1);
    assert_eq!(map.rows[0].object_index.value, Some(0x1234));
    assert_eq!(map.rows[0].introduced_state, 0x56);
    assert_eq!(map.rows[0].modified_state, 0x57);
    assert_eq!(map.rows[1].row_kind, 2);
    assert_eq!(map.rows[1].object_index.value, Some(0x31f85));
    assert_eq!(map.rows[1].introduced_state, 0x2a);
    assert_eq!(map.rows[1].modified_state, 0x2b);
    assert_eq!(
        map.rows[2].object_index.form,
        OperationStateIndexForm::Direct
    );
}

#[test]
fn operation_state_counter_map_rejects_a_short_non_suffix_lane() {
    let bytes = [
        0x05, 0x01, 0x12, 0x34, 0x56, 0x4e, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
    ];
    assert!(super::operation_state_counter_map(&bytes, 0).is_none());
}
