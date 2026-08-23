use std::collections::{HashMap, HashSet};

use super::super::{body_bridge::bridge_refs, disc14_bodies};
use super::record;

#[test]
fn disc14_body_membership_exposes_unique_bridge_slots() {
    let records = vec![
        record(10, 0x001a, [1, 20, 1, 1, 1, 1]),
        record(20, 0x0016, [1, 1, 1, 1, 1, 1]),
        record(30, 0x0020, [1, 1, 40, 1, 1, 1]),
        record(31, 0x0020, [1, 1, 41, 1, 1, 1]),
        record(40, 0x0014, [100, 1, 1, 1, 1, 1]),
        record(41, 0x0014, [101, 1, 1, 1, 1, 1]),
    ]
    .into_iter()
    .map(|mut record| {
        if matches!(record.attr, 40 | 41) {
            record.flags = 2;
        }
        record
    })
    .collect::<Vec<_>>();
    let by_attr = records
        .iter()
        .map(|record| (record.attr, record))
        .collect::<HashMap<_, _>>();
    let bodies = disc14_bodies(&by_attr);
    let [body] = bodies.as_slice() else {
        panic!("one disc14 body");
    };
    assert_eq!(body.refs, [40, 41, 100, 101]);
    assert_eq!(body.regions[0].shells[0].refs, [40, 41, 100, 101]);

    let duplicate = [
        record(40, 0x0014, [100, 1, 1, 1, 1, 1]),
        record(41, 0x0014, [100, 1, 1, 1, 1, 1]),
    ];
    let duplicate_by_attr = duplicate
        .iter()
        .map(|record| (record.attr, record))
        .collect::<HashMap<_, _>>();
    assert!(bridge_refs(&duplicate_by_attr, &HashSet::from([40, 41])).is_none());
}
