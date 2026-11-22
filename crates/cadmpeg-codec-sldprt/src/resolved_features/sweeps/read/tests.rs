use super::*;
use crate::records::FeatureInputClass;
use crate::resolved_features::sweeps::tests::{lane, operand};

fn input(circular: bool, repeated: bool) -> (FeatureInputLane, usize, usize) {
    let mut bytes = vec![0; 32];
    let (contact, source, options) = operand(&mut bytes, circular, repeated);
    let mut lane = lane(bytes);
    lane.classes.push(FeatureInputClass {
        id: "contact".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: u64::try_from(contact).unwrap(),
        name: "moWireContact_c".into(),
    });
    (lane, source, options)
}

#[test]
fn declared_and_repeated_general_curve_path_slots_have_the_same_source() {
    for repeated in [false, true] {
        let (lane, _, _) = input(false, repeated);
        let path = path(&lane, 0, lane.native_payload.len()).unwrap();
        assert_eq!(path.source, Some(11));
        assert_eq!(path.radius_mm, None);
    }
}

#[test]
fn circular_section_diameter_is_converted_to_radius_in_millimetres() {
    let (lane, _, _) = input(true, false);
    assert_eq!(
        path(&lane, 0, lane.native_payload.len()).unwrap().radius_mm,
        Some(4.0)
    );
}

#[test]
fn malformed_source_keeps_the_path_identity() {
    let (mut lane, source, _) = input(true, false);
    lane.native_payload[source + 32] = 0;
    let operand = path(&lane, 0, lane.native_payload.len()).unwrap();
    assert_eq!(operand.source, None);
    assert_eq!(operand.radius_mm, None);
}

#[test]
fn unreadable_section_keeps_the_source_and_path_record() {
    let (mut lane, _, options) = input(true, false);
    lane.native_payload[options + 90..options + 98].copy_from_slice(&f64::NAN.to_le_bytes());
    let operand = path(&lane, 0, lane.native_payload.len()).unwrap();
    assert_eq!(operand.source, Some(11));
    assert_eq!(operand.radius_mm, None);
    assert!(path(&lane, 0, options).is_some());
}

#[test]
fn a_contact_outside_the_owner_cannot_supply_the_path() {
    let (lane, _, _) = input(false, false);
    assert!(path(&lane, 33, lane.native_payload.len()).is_none());
}

#[test]
fn a_counted_snapshot_ends_before_the_sweep_options() {
    let (mut lane, source, _) = input(true, false);
    let body_flag = source + 52 + 32;
    let mut payload = lane.native_payload.to_vec();
    payload[body_flag] = 1;
    let mut body = vec![0];
    body.extend_from_slice(&37u32.to_le_bytes());
    body.push(0);
    body.extend_from_slice(&32u32.to_le_bytes());
    body.extend_from_slice(&crate::layout::zlb_wrapper_header::MAGIC_VALUE);
    body.extend_from_slice(&[0; 16]);
    let insertion = body_flag + 1;
    payload.splice(insertion..insertion, body);
    lane.native_payload = payload.into();
    assert_eq!(
        path(&lane, 0, lane.native_payload.len()).unwrap().radius_mm,
        Some(4.0)
    );
    lane.native_payload[source + 52 + 33 + 6..source + 52 + 33 + 10]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    let path = path(&lane, 0, lane.native_payload.len()).unwrap();
    assert_eq!(path.source, Some(11));
    assert_eq!(path.radius_mm, None);
}

#[test]
fn an_extended_path_header_keeps_its_independent_word() {
    for header in [0u32, 17000, u32::MAX] {
        let (mut lane, source, _) = input(true, true);
        let mut payload = lane.native_payload.to_vec();
        payload.splice(source..source, header.to_le_bytes());
        lane.native_payload = payload.into();
        let operand = path(&lane, 0, lane.native_payload.len()).unwrap();
        assert_eq!(operand.source, Some(11));
        assert_eq!(operand.radius_mm, Some(4.0));
        assert!(operand.header_assumed);
    }
}
