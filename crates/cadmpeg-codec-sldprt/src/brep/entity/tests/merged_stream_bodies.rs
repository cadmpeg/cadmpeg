use super::super::scan_combined_bodies;
use crate::test_support::entity51;

const TEST_SCHEMA: &str = "SCH_SW_33103_11000";

#[test]
fn merged_streams_supply_faces_and_body_chain_to_one_layout() {
    let partition = [
        entity51(1, 20, 0x04, &[100, 30, 1, 1, 1, 1]),
        entity51(1, 21, 0x04, &[101, 31, 1, 1, 1, 1]),
        entity51(1, 30, 0x1a, &[100, 40, 20, 1, 1, 1]),
        entity51(1, 31, 0x1a, &[101, 41, 21, 1, 1, 1]),
        entity51(4, 40, 0x22, &[100, 1, 30, 1, 1, 1, 1, 1, 1]),
        entity51(4, 41, 0x22, &[101, 1, 31, 1, 1, 1, 1, 1, 1]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let deltas = [
        entity51(2, 10, 0x20, &[3, 1, 11, 1, 1, 1, 1]),
        entity51(2, 11, 0x12, &[3, 10, 12, 1, 1, 1, 1]),
        entity51(2, 12, 0x1e, &[3, 11, 13, 1, 1, 1, 1]),
        entity51(2, 13, 0x1c, &[3, 12, 14, 1, 1, 1, 1]),
        entity51(1, 14, 0x18, &[3, 13, 1, 1, 1, 1]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let (bodies, ambiguous) = scan_combined_bodies(&[
        (&partition, TEST_SCHEMA, false),
        (&deltas, TEST_SCHEMA, false),
    ]);
    assert_eq!(ambiguous, 0);
    let [body] = bodies.as_slice() else {
        panic!("one merged-stream body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn partition_chain_survives_prefixed_delta_updates() {
    let partition = [
        entity51(2, 100, 0x22, &[3, 1, 101, 1, 1, 1, 1]),
        entity51(2, 101, 0x1e, &[3, 100, 102, 1, 1, 1, 1]),
        entity51(2, 102, 0x1c, &[3, 101, 103, 1, 1, 1, 1]),
        entity51(2, 103, 0x1a, &[3, 102, 104, 1, 1, 1, 1]),
        entity51(2, 104, 0x18, &[3, 103, 105, 1, 1, 1, 1]),
        entity51(2, 105, 0x16, &[3, 104, 1, 1, 1, 1, 1]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let deltas = [
        prefixed_entity51(2, 100, 0x22, &[3, 1, 910, 1, 1, 1, 1]),
        prefixed_entity51(2, 101, 0x1e, &[3, 100, 911, 1, 1, 1, 1]),
        prefixed_entity51(2, 102, 0x1c, &[3, 101, 912, 1, 1, 1, 1]),
        prefixed_entity51(2, 103, 0x1a, &[3, 102, 913, 1, 1, 1, 1]),
        prefixed_entity51(2, 104, 0x18, &[3, 103, 914, 1, 1, 1, 1]),
        prefixed_entity51(2, 105, 0x16, &[3, 104, 1, 1, 1, 1, 1]),
        prefixed_entity51(1, 200, 0x04, &[3, 102, 1, 1, 1, 900]),
        prefixed_entity51(1, 201, 0x04, &[3, 102, 1, 1, 1, 901]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let (bodies, ambiguous) = scan_combined_bodies(&[
        (&partition, TEST_SCHEMA, false),
        (&deltas, TEST_SCHEMA, true),
    ]);
    assert_eq!(ambiguous, 0);
    let [body] = bodies.as_slice() else {
        panic!("one body from the partition chain and delta faces");
    };
    assert_eq!(body.attr, 100);
    assert_eq!(body.regions[0].shells[0].attr, 102);
    assert!(body.refs.contains(&200) && body.refs.contains(&201));
}

fn prefixed_entity51(flags: u32, attr: u16, disc: u16, slots: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&flags.to_be_bytes());
    bytes.extend_from_slice(&attr.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&disc.to_be_bytes());
    for slot in slots {
        bytes.push(1);
        bytes.extend_from_slice(&slot.to_be_bytes());
    }
    bytes.push(0);
    bytes
}
