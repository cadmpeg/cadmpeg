use super::*;

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn scope(long: bool) -> Vec<u8> {
    let mut bytes = vec![0; 25];
    bytes[20] = 1;
    bytes[21..25].copy_from_slice(&42u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&reference(12));
    bytes.extend_from_slice(&reference(11));
    bytes.extend_from_slice(&[255, 255, 255, 255, 0, 0]);
    if long {
        bytes.extend_from_slice(&9u32.to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&reference(14));
    bytes.extend_from_slice(&reference(13));
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&36u32.to_le_bytes());
    for word in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn read(bytes: &[u8]) -> Result<MeshConversionScope, MeshOperationFraming> {
    crate::test_support::with_decode_context(|ctx| read_scope(ctx, bytes)).unwrap()
}

#[test]
fn conversion_lists_keep_input_and_output_order_in_both_envelopes() {
    for long in [false, true] {
        let parsed = read(&scope(long)).unwrap();
        assert_eq!(parsed.prefix_word, Some(42));
        assert_eq!(parsed.control, Some(3));
        assert_eq!(parsed.input_count, Some(2));
        assert_eq!(parsed.input_wrappers, [12, 11]);
        assert_eq!(parsed.output_wrappers, [14, 13]);
        assert_eq!(parsed.tail_control, long.then_some(9));
        assert_eq!(parsed.tail_error, None);
        assert!(parsed.deviations.is_empty());
    }
}

#[test]
fn conversion_scope_flag_moves_the_control_and_count_together() {
    let mut bytes = scope(false);
    bytes.drain(21..25);
    bytes[20] = 0;
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.prefix_word, None);
    assert_eq!(parsed.control, Some(3));
    assert_eq!(parsed.input_wrappers, [12, 11]);
    assert_eq!(parsed.output_wrappers, [14, 13]);
    assert_eq!(parsed.tail_error, None);
}

#[test]
fn conversion_null_slots_keep_later_input_and_output_references() {
    let mut bytes = scope(false);
    bytes.splice(33..44, [0]);
    bytes.splice(55..66, [0]);
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.input_wrappers, [0, 11]);
    assert_eq!(parsed.output_wrappers, [0, 13]);
    assert_eq!(parsed.tail_error, None);
}

#[test]
fn conversion_controls_and_inputs_survive_a_truncated_output_tail() {
    let bytes = scope(false);
    for length in [55, 58, 60, 64, 70, 90] {
        let parsed = read(&bytes[..length]).unwrap();
        assert_eq!(parsed.control, Some(3));
        assert_eq!(parsed.input_wrappers, [12, 11]);
        assert!(parsed.tail_error.is_some());
    }
}

#[test]
fn conversion_prefix_word_survives_a_missing_method_control() {
    let bytes = scope(false);
    let parsed = read(&bytes[..25]).unwrap();
    assert_eq!(parsed.prefix_word, Some(42));
    assert_eq!(parsed.control, None);
    assert!(parsed.input_wrappers.is_empty());
    assert_eq!(
        parsed.tail_error,
        Some(MeshOperationFraming("conversion control"))
    );
}

#[test]
fn conversion_input_count_is_physically_bounded_before_allocation() {
    let mut bytes = scope(false);
    bytes[29..33].copy_from_slice(&4_294_967_295u32.to_le_bytes());
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.control, Some(3));
    assert_eq!(parsed.input_count, Some(4_294_967_295));
    assert!(parsed.input_wrappers.is_empty());
    assert_eq!(
        parsed.tail_error,
        Some(MeshOperationFraming("conversion input count"))
    );
}

#[test]
fn conversion_bad_reference_keeps_preceding_inputs() {
    let mut bytes = scope(false);
    bytes[44] = 9;
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.input_wrappers, [12, 0]);
    assert_eq!(
        parsed.tail_error,
        Some(MeshOperationFraming("conversion input wrapper reference"))
    );
}

#[test]
fn conversion_envelopes_require_the_following_counted_scope_guid() {
    let mut bytes = scope(false);
    bytes[95..99].copy_from_slice(&35u32.to_le_bytes());
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.input_wrappers, [12, 11]);
    assert!(parsed.output_wrappers.is_empty());
    assert_eq!(
        parsed.tail_error,
        Some(MeshOperationFraming("conversion output envelope"))
    );
}

#[test]
fn conversion_output_anchor_requires_a_guid_value() {
    let mut bytes = scope(false);
    bytes[99] = b'G';
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.input_wrappers, [12, 11]);
    assert_eq!(
        parsed.tail_error,
        Some(MeshOperationFraming("conversion output envelope"))
    );
}

#[test]
fn conversion_reserved_values_are_retained_as_deviations() {
    let mut bytes = scope(false);
    bytes[55..59].copy_from_slice(&7u32.to_le_bytes());
    bytes[59] = 6;
    let parsed = read(&bytes).unwrap();
    assert_eq!(
        parsed.deviations,
        [("conversion-tail-word", 7), ("conversion-tail-byte:0", 6)]
    );
    assert_eq!(parsed.output_wrappers, [14, 13]);
}

#[test]
fn conversion_prefix_refusals_are_local_framing_errors() {
    let mut bytes = scope(false);
    bytes[20] = 2;
    assert_eq!(
        read(&bytes),
        Err(MeshOperationFraming("conversion scope flag"))
    );
    bytes[20] = 1;
    bytes[15] = 4;
    assert_eq!(
        read(&bytes),
        Err(MeshOperationFraming("conversion scope prefix"))
    );
    assert!(read(&bytes[..11]).is_err());
}
