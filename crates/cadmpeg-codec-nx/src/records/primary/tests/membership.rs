// SPDX-License-Identifier: Apache-2.0

use super::{header, with_decode_context, PrimaryRecords, END};
use crate::records::group::MembershipKind;

fn member(first: bool, label: u32) -> Vec<u8> {
    let mut bytes = 91u16.to_be_bytes().to_vec();
    if first {
        bytes.push(255);
    }
    bytes.extend_from_slice(&12u16.to_be_bytes());
    bytes.extend_from_slice(&label.to_be_bytes());
    for reference in [10u16, 100, 3, 4, 20, 1] {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes
}

#[test]
fn primary_membership_uses_plain_fields_and_full_integer_label_storage() {
    with_decode_context(|ctx| {
        for version in [11004, 13006] {
            for embedded in [false, true] {
                for label in [0, 2, u32::MAX] {
                    let mut bytes = header(version, embedded);
                    bytes.extend(member(embedded, label));
                    let end = bytes.len();
                    bytes.extend(END);
                    let primary = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
                    let records = primary.membership_records(ctx).unwrap();
                    assert_eq!(records.len(), 1);
                    assert_eq!(u32::from(records[0].xmt), 12);
                    assert_eq!(
                        records[0].kind,
                        MembershipKind::Member {
                            owning_group: 10,
                            owner: 100,
                            next_member: 20,
                            previous_member: 1,
                        }
                    );
                    assert_eq!(primary.records[0].range.end, end);
                    assert_eq!(
                        &bytes[primary.records[0].fields[0].clone()],
                        &label.to_be_bytes()
                    );
                    let incomplete = PrimaryRecords::read(ctx, &bytes[..end - 1])
                        .unwrap()
                        .unwrap();
                    assert!(incomplete.membership_records(ctx).unwrap().is_empty());
                }
            }
        }
    });
}

#[test]
fn primary_membership_does_not_scan_member_tags_inside_array_storage() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, false);
        let fake = member(false, 2);
        assert_eq!(fake.len() % 2, 0);
        bytes.extend_from_slice(&96u16.to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(fake.len() / 2).unwrap().to_be_bytes());
        bytes.extend_from_slice(&50u16.to_be_bytes());
        bytes.extend(fake);
        bytes.extend(END);
        let primary = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        let records = primary.membership_records(ctx).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(u32::from(records[0].xmt), 50);
        assert_eq!(records[0].kind, MembershipKind::Other);
    });
}

#[test]
fn primary_membership_preserves_duplicate_source_identities_for_quarantine() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, false);
        bytes.extend(member(false, 0));
        bytes.extend(member(false, 2));
        bytes.extend(END);
        let primary = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        let records = primary.membership_records(ctx).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].xmt, records[1].xmt);
    });
}

#[test]
fn primary_membership_instance_is_type_eleven_with_a_node_label() {
    with_decode_context(|ctx| {
        for version in [11004, 13006] {
            let mut bytes = header(version, false);
            bytes.extend_from_slice(&11u16.to_be_bytes());
            bytes.extend_from_slice(&100u16.to_be_bytes());
            bytes.extend_from_slice(&42u32.to_be_bytes());
            bytes.extend_from_slice(&1u16.to_be_bytes());
            bytes.push(0);
            for _ in 0..7 {
                bytes.extend_from_slice(&1u16.to_be_bytes());
            }
            bytes.extend(END);
            let primary = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
            let records = primary.membership_records(ctx).unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(
                records[0].kind,
                MembershipKind::Node {
                    kind: 11,
                    node_id: 42
                }
            );
        }
    });
}
