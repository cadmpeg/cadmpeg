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
