// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::history_records::{AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmStateSnapshot};

fn record(attribute: i64, carrier: i64) -> Record {
    Record {
        index: 1,
        name: "face".into(),
        tokens: vec![
            Token::Ref(attribute),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(carrier),
            Token::False,
            Token::False,
        ]
        .into(),
        offset: 0,
        len: 0,
    }
}

#[test]
fn transition_geometry_compares_updated_record_content() {
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        ..Default::default()
    };
    let older = BTreeMap::from([(1, 10)]);
    let newer = BTreeMap::from([(1, 11)]);
    let mut transition = AsmHistoricalTransition::between(
        Some(AsmStateSnapshot {
            state_id: 0,
            topology: &topology,
            versions: &older,
        }),
        AsmStateSnapshot {
            state_id: 1,
            topology: &topology,
            versions: &newer,
        },
    )
    .unwrap();
    let mut archive = HistoricalRecordArchive::from([(10, record(20, 30)), (11, record(21, 30))]);
    assert!(unchanged(&transition, &older, &newer, &archive));
    archive.insert(11, record(21, 31));
    assert!(!unchanged(&transition, &older, &newer, &archive));
    archive.remove(&10);
    assert!(!unchanged(&transition, &older, &newer, &archive));
    transition.topology.faces = AsmHistoricalEntityDelta {
        inserted: vec![1],
        ..Default::default()
    };
    assert!(!unchanged(&transition, &older, &newer, &archive));
}

#[test]
fn transition_geometry_compares_positions_parameters_and_senses() {
    for (old, new) in [
        (
            Token::Position([0.0, 2.0, 3.0]),
            Token::Position([1.0, 2.0, 3.0]),
        ),
        (Token::Vector2([0.0, 1.0]), Token::Vector2([0.0, 2.0])),
        (Token::Double(1.0), Token::Double(2.0)),
        (Token::False, Token::True),
        (Token::Ref(2), Token::Ref(3)),
    ] {
        let mut before = record(20, 30);
        let mut after = record(21, 30);
        before.tokens = vec![Token::Ref(20), old].into();
        after.tokens = vec![Token::Ref(21), new].into();
        assert!(!same_content(&before, &after));
    }
}

#[test]
fn transition_geometry_requires_matching_record_type_and_prefix() {
    let before = record(20, 30);
    let mut after = record(21, 30);
    after.name = "edge".into();
    assert!(!same_content(&before, &after));
    after.name = "face".into();
    after.tokens = before.tokens[1..].to_vec().into();
    assert!(!same_content(&before, &after));
}
