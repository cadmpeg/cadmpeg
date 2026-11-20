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

fn named(name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index: 1,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn free_edge(coedge: i64) -> Record {
    named(
        "edge",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(6),
            Token::Double(0.0),
            Token::Ref(7),
            Token::Double(1.0),
            Token::Ref(coedge),
            Token::Ref(8),
            Token::False,
            Token::Str("unknown".into()),
        ],
    )
}

fn free_vertex(point: i64) -> Record {
    named(
        "vertex",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(5),
            Token::Ref(point),
        ],
    )
}

fn free_edge_topology(extra_vertex: bool) -> AsmHistoricalTopology {
    let mut vertices = vec![6, 7];
    if extra_vertex {
        vertices.push(11);
    }
    AsmHistoricalTopology {
        bodies: vec![5],
        regions: vec![5],
        shells: vec![5],
        faces: vec![1],
        edges: vec![5],
        vertices,
        curves: vec![8],
        points: vec![9, 10],
        ..Default::default()
    }
}

fn free_edge_archive(coedge: i64, carrier: i64) -> HistoricalRecordArchive {
    HistoricalRecordArchive::from([
        (10, record(20, 30)),
        (11, record(21, carrier)),
        (50, free_edge(coedge)),
        (60, free_vertex(9)),
        (70, free_vertex(10)),
        (
            80,
            named(
                "straight-curve",
                vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Position([0.0, 0.0, 0.0]),
                    Token::Position([1.0, 0.0, 0.0]),
                ],
            ),
        ),
        (
            90,
            named(
                "point",
                vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Position([0.0, 0.0, 0.0]),
                ],
            ),
        ),
        (
            100,
            named(
                "point",
                vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Position([1.0, 0.0, 0.0]),
                ],
            ),
        ),
        (110, free_vertex(9)),
    ])
}

fn free_edge_versions(extra_vertex: bool) -> BTreeMap<i64, i64> {
    let mut versions = BTreeMap::from([
        (1, 11),
        (5, 50),
        (6, 60),
        (7, 70),
        (8, 80),
        (9, 90),
        (10, 100),
    ]);
    if extra_vertex {
        versions.insert(11, 110);
    }
    versions
}

fn free_edge_unchanged(insert: bool, extra_vertex: bool, coedge: i64, carrier: i64) -> bool {
    let faces = AsmHistoricalTopology {
        faces: vec![1],
        ..Default::default()
    };
    let with_edge = free_edge_topology(extra_vertex);
    let face_versions = BTreeMap::from([(1, 10)]);
    let edge_versions = free_edge_versions(extra_vertex);
    let (older, newer) = if insert {
        ((&faces, &face_versions), (&with_edge, &edge_versions))
    } else {
        ((&with_edge, &edge_versions), (&faces, &face_versions))
    };
    let transition = AsmHistoricalTransition::between(
        Some(AsmStateSnapshot {
            state_id: 0,
            topology: older.0,
            versions: older.1,
        }),
        AsmStateSnapshot {
            state_id: 1,
            topology: newer.0,
            versions: newer.1,
        },
    )
    .unwrap();
    unchanged(
        &transition,
        older.1,
        newer.1,
        &free_edge_archive(coedge, carrier),
    )
}

#[test]
fn transition_geometry_admits_inserted_saved_free_edges() {
    assert!(free_edge_unchanged(true, false, -1, 30));
}

#[test]
fn transition_geometry_refuses_geometric_change_beside_saved_free_edges() {
    // A face carrier change still refuses the transition.
    assert!(!free_edge_unchanged(true, false, -1, 31));
    // An edge that a coedge uses belongs to a face loop.
    assert!(!free_edge_unchanged(true, false, 3, 30));
    // An inserted vertex that no inserted free edge uses.
    assert!(!free_edge_unchanged(true, true, -1, 30));
    // Deleted saved free edges stay refused.
    assert!(!free_edge_unchanged(false, false, -1, 30));
}
