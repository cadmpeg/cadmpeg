// SPDX-License-Identifier: Apache-2.0

use super::super::{operation_states, operations};

/// Two `mtrack_record_ptr` rows with `modif_time` 2014-12-04 08:38:39.
const TIMESTAMPED_ROWS: &[u8] = b"\xe1C183\0\xe2\x27\x26\x08\x04\x0brProtrusion id 5940\0\
    \xf6\x88\x04\x08\0\xe4\x84\x04\xe1\x89\xf0\xe1C183\0\xe2\x27\x26\x08\x04\x0brDatum Plane id 5941\0";

#[test]
fn item_name_starts_after_the_timestamp_year() {
    let states = operation_states(TIMESTAMPED_ROWS);
    assert_eq!(states.len(), 2);
    for (state, (feature_id, family, name)) in states.iter().zip([
        (5940, "Protrusion", "Protrusion id 5940"),
        (5941, "Datum Plane", "Datum Plane id 5941"),
    ]) {
        assert_eq!(state.feature_id, feature_id);
        assert_eq!(state.kind.as_str(), family);
        assert_eq!(state.stored_name().as_deref(), Some(name));
        assert_eq!(
            TIMESTAMPED_ROWS.get(state.offset..state.offset + name.len()),
            Some(name.as_bytes())
        );
    }
}

#[test]
fn two_byte_year_ends_the_timestamp() {
    let states = operation_states(b"\xe2\x01\x02\x03\x04\x05\x80\x80Round id 9\0");
    let [state] = states.as_slice() else {
        panic!("one timestamped state");
    };
    assert_eq!(state.kind.as_str(), "Round");
    assert_eq!(state.offset, 8);
}

#[test]
fn name_without_a_timestamp_keeps_its_leading_letter() {
    let states = operation_states(b"\xe3xExtrude id 7\0\xe3Extrude ID 7\0");
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].kind.as_str(), "xExtrude");
    assert_eq!(states[0].stored_name().as_deref(), Some("xExtrude id 7"));
    assert_eq!(states[0].offset, 1);
    assert_eq!(states[1].kind.as_str(), "Extrude");
    assert_eq!(states[1].name.identifier_keyword(), Some("ID"));
    assert!(operations(b"\xe3xExtrude id 7\0\xe3Extrude ID 7\0").is_empty());
}

#[test]
fn out_of_range_day_is_not_a_timestamp() {
    let states = operation_states(b"\xe2\x01\x02\x03\x00\x05rRound id 9\0");
    let [state] = states.as_slice() else {
        panic!("one state");
    };
    assert_eq!(state.kind.as_str(), "rRound");
}
