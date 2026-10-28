use crate::layout::b5_plane_payload;
use std::collections::BTreeSet;

use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;

use super::super::B5Record;
use super::from_records;

fn record(class: u8, object_id: u32, payload: Vec<u8>) -> B5Record {
    B5Record {
        offset: 0,
        family: 0xb5,
        class,
        object_id,
        payload,
    }
}

fn plane(object_id: u32) -> B5Record {
    let mut payload = vec![0; b5_plane_payload::LEN];
    payload[b5_plane_payload::LEAD] = 0x80;
    for (offset, values) in [
        (b5_plane_payload::DIRECTION_U, [0.0_f64, 1.0, 0.0]),
        (b5_plane_payload::DIRECTION_V, [0.0_f64, 0.0, 1.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            payload[offset + 8 * index..offset + 8 * (index + 1)]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for offset in [
        b5_plane_payload::FIXED_VALUES,
        b5_plane_payload::FIXED_VALUES + 8,
        b5_plane_payload::U_RANGE + 8,
        b5_plane_payload::V_RANGE + 8,
    ] {
        payload[offset..offset + 8].copy_from_slice(&1.0_f64.to_le_bytes());
    }
    record(0x27, object_id, payload)
}

fn complete_owner() -> Vec<B5Record> {
    vec![
        record(0x65, 10, vec![0x81, 0x8b, 0x05, 0x0d]),
        record(0x60, 11, vec![0x81, 0x8c]),
        record(
            0x61,
            12,
            vec![0x82, 0x94, 0x95, 0x82, 0x05, 0xff, 0xff, 0x01, 0x00, 0x03],
        ),
        record(0x5f, 20, vec![0x82, 0x9e, 0xa8, 0x03]),
        record(0x5f, 21, vec![0x82, 0x9f, 0xa9, 0x05]),
        plane(30),
        plane(31),
    ]
}

#[test]
fn selected_shell_members_compose_both_plane_chart_parities() {
    let records = complete_owner();
    let selected =
        from_records(&records, &BTreeSet::from([20, 21])).expect("unique selected owner");
    let negative_x = UnitVector3::new(Vector3::new(-1.0, 0.0, 0.0)).expect("negative unit X");
    assert_eq!(selected[&20].stored_member, -1);
    assert_eq!(selected[&21].stored_member, 1);
    assert_eq!(
        selected[&20].forward_on(UnitVector3::X_AXIS, true),
        Some(true)
    );
    assert_eq!(selected[&20].forward_on(negative_x, true), Some(false));
    assert_eq!(
        selected[&21].forward_on(UnitVector3::X_AXIS, false),
        Some(false)
    );
    assert_eq!(selected[&21].forward_on(negative_x, false), Some(true));
    assert_eq!(selected[&20].forward_on(UnitVector3::X_AXIS, false), None);
    assert_eq!(selected[&20].forward_on(UnitVector3::Y_AXIS, true), None);
}

#[test]
fn selected_owner_requires_exact_domain_and_unique_root() {
    let mut records = complete_owner();
    assert!(from_records(&records, &BTreeSet::from([20])).is_none());
    records.push(record(0x65, 13, vec![0x81, 0x8b, 0x05, 0x0d]));
    assert!(from_records(&records, &BTreeSet::from([20, 21])).is_none());
}

#[test]
fn malformed_roster_lanes_never_supply_a_shell_orientation() {
    for (offset, replacement) in [(3, 0x81), (4, 0x07), (5, 0x00), (9, 0x05), (2, 0x94)] {
        let mut records = complete_owner();
        records[2].payload[offset] = replacement;
        assert!(from_records(&records, &BTreeSet::from([20, 21])).is_none());
    }
    let mut records = complete_owner();
    records[2].payload.push(0);
    assert!(from_records(&records, &BTreeSet::from([20, 21])).is_none());
    let mut records = complete_owner();
    records[1].payload.push(0);
    assert!(from_records(&records, &BTreeSet::from([20, 21])).is_none());
}

#[test]
fn duplicate_or_wrong_class_reference_abstains() {
    let mut records = complete_owner();
    records.push(records[3].clone());
    assert!(from_records(&records, &BTreeSet::from([20, 21])).is_none());
    let mut records = complete_owner();
    records[2].class = 0x60;
    assert!(from_records(&records, &BTreeSet::from([20, 21])).is_none());
}
