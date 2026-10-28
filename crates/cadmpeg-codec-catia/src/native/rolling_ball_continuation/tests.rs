use super::{derive, fields, CatiaRollingBallContinuation, ScalarJet};
use crate::test_support::test_rolling_ball_continuation::{fixture, TailForm};
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth};

fn decode(bytes: &[u8]) -> Vec<CatiaRollingBallContinuation> {
    let records = crate::wire::records::consolidated_records(bytes);
    crate::test_support::with_service_context(|ctx| derive(ctx, bytes, &records))
        .expect("continuation admission")
}

fn set_word(bytes: &mut [u8], offset: usize, value: f64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn resize_frame(bytes: &mut [u8]) {
    let length = u32::try_from(bytes.len() - 8).expect("payload size");
    bytes[3..7].copy_from_slice(&length.to_le_bytes());
}

#[test]
fn continuation_keeps_current_and_maximal_limits_as_exact_words() {
    let (bytes, tail) = fixture(2, false, TailForm::Absent79);
    let row = decode(&bytes).remove(0);
    assert_eq!(row.site_count, 2);
    assert_eq!(row.tail_offset, u64::try_from(tail).expect("offset"));
    assert_eq!(row.current_u_bits, [0.25_f64.to_bits(), 0.75_f64.to_bits()]);
    assert_eq!(row.maximal_u_bits, [0.0_f64.to_bits(), 1.0_f64.to_bits()]);
    assert_eq!(
        row.current_v_bits,
        [(-0.0_f64).to_bits(), 2.0_f64.to_bits()]
    );
    assert_eq!(
        row.maximal_v_bits,
        [(-0.1_f64).to_bits(), 2.1_f64.to_bits()]
    );
    assert_eq!(row.reference_scalar_bits, 2.0_f64.to_bits());
    assert_eq!(row.raw_control, 3);
    assert_eq!(&*row.frame.payload, &bytes[8..]);
}

#[test]
fn continuation_distinguishes_absent_79_and_87_byte_forms() {
    for (form, length, repeated) in [
        (TailForm::Absent79, 79, None),
        (TailForm::Absent87, 87, Some(2.0_f64.to_bits())),
    ] {
        let (mut bytes, tail) = fixture(2, false, form);
        assert_eq!(bytes.len() - tail, length);
        for control in [3, 5] {
            bytes[tail + crate::layout::rolling_ball_tail_prefix::CONTROL] = control;
            let row = decode(&bytes).remove(0);
            assert_eq!(row.raw_control, control);
            assert_eq!(row.reference_scalar_repeat_bits, repeated);
            assert_eq!(row.scalar_jet, ScalarJet::Absent);
        }
    }
}

#[test]
fn continuation_preserves_explicit_scalar_lanes_and_terminal() {
    let (mut bytes, tail) = fixture(2, false, TailForm::Explicit);
    let terminal_offset = tail + crate::layout::rolling_ball_tail_prefix::LEN + 2 + 6 * 8;
    for terminal in [0u32, 1] {
        bytes[terminal_offset..terminal_offset + 4].copy_from_slice(&terminal.to_le_bytes());
        let row = decode(&bytes).remove(0);
        assert_eq!(
            row.scalar_jet,
            ScalarJet::Explicit {
                values_bits: vec![2.0_f64.to_bits(), 1.0_f64.to_bits()],
                first_derivative_bits: vec![(-1.0_f64).to_bits(); 2],
                second_derivative_bits: vec![(-0.0_f64).to_bits(); 2],
                terminal,
            }
        );
        assert_eq!(row.reference_scalar_repeat_bits, Some(2.0_f64.to_bits()));
    }
}

#[test]
fn continuation_uses_actual_encoded_prefix_and_marker_widths() {
    for (count, prefix) in [(2u32, 3usize), (64, 7)] {
        for wide_marker in [false, true] {
            let (bytes, tail) = fixture(count, wide_marker, TailForm::Explicit);
            let row = decode(&bytes).remove(0);
            if count == 64 {
                assert_eq!(&bytes[8..15], &[0x08, 64, 0, 21, 0x08, 64, 0]);
            }
            let marker = if wide_marker { 2 } else { 1 };
            assert_eq!(
                tail,
                8 + prefix + marker + usize::try_from(count).expect("count") * 31 * 8
            );
            assert_eq!(row.site_count, u64::from(count));
            assert_eq!(row.tail_offset, u64::try_from(tail).expect("offset"));
        }
    }
}

#[test]
fn continuation_retains_independent_header_width_flag_and_raw_token() {
    let (base, tail) = fixture(2, false, TailForm::Absent79);
    for (width, token, encoded) in [
        (1u8, 5u32, ConsolidatedFrameWidth::One),
        (2, 0x1234, ConsolidatedFrameWidth::Two),
        (3, 0x0012_3456, ConsolidatedFrameWidth::Three),
    ] {
        for (flag, expected) in [
            (3, ConsolidatedFrameFlag::Flag03),
            (19, ConsolidatedFrameFlag::Flag13),
            (131, ConsolidatedFrameFlag::Flag83),
        ] {
            let mut bytes = base[..7].to_vec();
            bytes[0] = 0xa4 + width;
            bytes[1] = flag;
            bytes.extend_from_slice(&token.to_le_bytes()[..usize::from(width)]);
            bytes.extend_from_slice(&base[8..]);
            let row = decode(&bytes).remove(0);
            assert_eq!(row.frame.width(), encoded);
            assert_eq!(row.frame.flag, expected);
            assert_eq!(row.frame.header_token(), token);
            assert_eq!(
                row.tail_offset,
                u64::try_from(tail + usize::from(width) - 1).expect("offset")
            );
        }
    }
    let mut invalid = base;
    invalid[7] = 2;
    assert!(decode(&invalid).is_empty());
}

#[test]
fn continuation_rejects_nonfinite_disordered_and_inconsistent_limits() {
    let (base, tail) = fixture(2, false, TailForm::Absent87);
    for (offset, value) in [
        (12, f64::NAN),
        (20, 0.0),
        (28, f64::INFINITY),
        (tail, 0.75),
        (tail + 8, 2.0),
        (tail + 16, 2.0),
        (tail + 32, 3.0),
        (tail + 41, 0.5),
        (tail + 49, 1.5),
        (tail + 63, -1.0),
        (tail + 71, 2.0),
        (tail + 79, 3.0),
    ] {
        let mut bytes = base.clone();
        set_word(&mut bytes, offset, value);
        assert!(decode(&bytes).is_empty(), "invalid word at {offset}");
    }
    let mut bytes = base;
    bytes[tail + crate::layout::rolling_ball_tail_prefix::CONTROL] = 1;
    assert!(decode(&bytes).is_empty());
}

#[test]
fn continuation_rejects_incomplete_branches_terminals_and_extra_bytes() {
    for form in [TailForm::Absent79, TailForm::Absent87, TailForm::Explicit] {
        let (base, tail) = fixture(2, false, form);
        let mut invalid = base.clone();
        invalid[tail + crate::layout::rolling_ball_tail_prefix::LEN] = 0xff;
        assert!(decode(&invalid).is_empty());
        let mut truncated = base.clone();
        truncated.pop();
        resize_frame(&mut truncated);
        assert!(decode(&truncated).is_empty());
        let mut extra = base;
        extra.push(0);
        resize_frame(&mut extra);
        assert!(decode(&extra).is_empty());
    }
    let (mut bytes, tail) = fixture(2, false, TailForm::Explicit);
    let terminal = tail + crate::layout::rolling_ball_tail_prefix::LEN + 2 + 6 * 8;
    bytes[terminal..terminal + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(decode(&bytes).is_empty());
    set_word(&mut bytes, tail + 59, f64::NAN);
    assert!(fields(&bytes[8..]).is_none());
}

#[test]
fn continuation_wire_authenticates_every_redundant_word_and_offset() {
    let (bytes, _) = fixture(2, false, TailForm::Explicit);
    let row = decode(&bytes).remove(0);
    let wire = serde_json::to_value(&row).expect("wire");
    assert_eq!(
        serde_json::from_value::<CatiaRollingBallContinuation>(wire.clone()).expect("checked wire"),
        row
    );
    for field in [
        "site_count",
        "tail_offset",
        "reference_scalar_bits",
        "reference_scalar_repeat_bits",
        "raw_control",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<CatiaRollingBallContinuation>(invalid).is_err(),
            "{field}"
        );
    }
    for field in [
        "current_u_bits",
        "current_v_bits",
        "maximal_u_bits",
        "maximal_v_bits",
    ] {
        for slot in 0..2 {
            let mut invalid = wire.clone();
            let actual = invalid[field][slot].as_u64().expect("word");
            invalid[field][slot] = serde_json::json!(actual ^ 1);
            assert!(serde_json::from_value::<CatiaRollingBallContinuation>(invalid).is_err());
        }
    }
    for field in [
        "values_bits",
        "first_derivative_bits",
        "second_derivative_bits",
    ] {
        let mut invalid = wire.clone();
        invalid["scalar_jet"][field][0] = serde_json::json!(0);
        assert!(serde_json::from_value::<CatiaRollingBallContinuation>(invalid).is_err());
    }
    for field in ["terminal", "kind", "header_token", "payload"] {
        let mut invalid = wire.clone();
        match field {
            "terminal" => invalid["scalar_jet"][field] = serde_json::json!(0),
            "kind" => invalid["scalar_jet"] = serde_json::json!({"kind":"absent"}),
            "header_token" => invalid["frame"][field] = serde_json::json!(256),
            "payload" => invalid["frame"][field] = serde_json::json!(""),
            _ => unreachable!(),
        }
        assert!(serde_json::from_value::<CatiaRollingBallContinuation>(invalid).is_err());
    }
}

#[test]
fn continuation_source_indices_keep_logical_and_physical_frame_identity() {
    let (frame, _) = fixture(2, false, TailForm::Absent79);
    let mut bytes = vec![0xff; 3];
    bytes.extend_from_slice(&frame);
    let second = bytes.len();
    bytes.extend_from_slice(&frame);
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [Some(3..second), Some(second..bytes.len())],
    );
    let rows = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes, &records))
        .expect("source admission");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].source_index,
            rows[0].source_offset,
            rows[0].frame.pos
        ),
        (0, 0, 3)
    );
    assert_eq!(
        (
            rows[1].source_index,
            rows[1].source_offset,
            rows[1].frame.pos
        ),
        (1, 0, u64::try_from(second).expect("offset"))
    );
    super::validate(&rows).expect("strict source order");
}

#[test]
fn continuation_spanning_frame_stays_source_only() {
    let (frame, _) = fixture(2, false, TailForm::Absent79);
    let mut bytes = vec![0xa5, 3, 0x24, 0, 0, 0, 0, 5];
    bytes.extend_from_slice(&frame);
    let split = 20;
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [vec![0..split, split..bytes.len()]],
    );
    assert!(records
        .iter()
        .any(|r| r.class() == 0x32 && r.range().is_none()));
    let rows = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes, &records))
        .expect("bounded admission");
    assert!(rows.is_empty());
}
