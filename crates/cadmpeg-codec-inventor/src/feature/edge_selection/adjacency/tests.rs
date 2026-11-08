// SPDX-License-Identifier: Apache-2.0
use super::*;

fn record(index: usize, name: &str, refs: &[(usize, i64)], len: usize) -> Record {
    let mut tokens = vec![Token::Ref(-1); len];
    for (slot, value) in refs {
        tokens[*slot] = Token::Ref(*value);
    }
    Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn edge(index: usize, coedge: i64) -> Record {
    record(index, "edge", &[(7, coedge)], 11)
}

fn coedge(index: usize, partner: i64, edge: i64, owner_loop: i64) -> Record {
    record(
        index,
        "coedge",
        &[(5, partner), (6, edge), (8, owner_loop)],
        11,
    )
}

fn owner_loop(index: usize, face: i64) -> Record {
    record(index, "loop", &[(5, face)], 6)
}

fn face(index: usize) -> Record {
    record(index, "face", &[], 9)
}

/// An edge 1 with coedges 2 and 3, loops 4 and 5, and faces 6 and 7.
fn ring() -> Vec<Record> {
    vec![
        edge(1, 2),
        coedge(2, 3, 1, 4),
        coedge(3, 2, 1, 5),
        owner_loop(4, 6),
        owner_loop(5, 7),
        face(6),
        face(7),
    ]
}

fn faces_of(records: &[Record]) -> Option<[i64; 2]> {
    let by_index = records
        .iter()
        .map(|record| (i64::try_from(record.index).expect("index"), record))
        .collect::<HashMap<_, _>>();
    faces(1, &by_index)
}

#[test]
fn a_two_coedge_ring_names_the_faces_of_the_edge_in_coedge_order() {
    assert_eq!(faces_of(&ring()), Some([6, 7]));
    let mut records = ring();
    records[0] = edge(1, 3);
    assert_eq!(faces_of(&records), Some([7, 6]));
}

#[test]
fn the_first_snapshot_record_may_carry_the_history_section_wrapper() {
    let mut records = ring();
    records[0].name = "End-of-ASM-History-Section".into();
    let mut tokens = vec![Token::Ident("edge".into())];
    tokens.extend(records[0].tokens.iter().cloned());
    records[0].tokens = tokens.into();
    assert_eq!(faces_of(&records), Some([6, 7]));
    // A wrapper around another record kind is no edge.
    records[0].tokens = vec![Token::Ident("vertex".into())]
        .into_iter()
        .chain(records[0].tokens.iter().skip(1).cloned())
        .collect::<Vec<_>>()
        .into();
    assert_eq!(faces_of(&records), None);
}

#[test]
fn any_other_ring_or_record_form_has_no_face_pair() {
    type Edit = fn(&mut Vec<Record>);
    let edits: [(&str, Edit); 9] = [
        ("one coedge", |records| records[1] = coedge(2, 2, 1, 4)),
        ("three coedges", |records| {
            records[1] = coedge(2, 3, 1, 4);
            records[2] = coedge(3, 8, 1, 5);
            records.push(coedge(8, 2, 1, 4));
        }),
        ("one face on both sides", |records| {
            records[4] = owner_loop(5, 6);
        }),
        ("coedge of another edge", |records| {
            records[2] = coedge(3, 2, 9, 5);
        }),
        ("open ring", |records| records[2] = coedge(3, -1, 1, 5)),
        ("loop is not a loop", |records| records[3] = face(4)),
        ("owner is not a face", |records| {
            records[6] = owner_loop(7, 6);
        }),
        ("no first coedge", |records| records[0] = edge(1, -1)),
        ("not an edge", |records| records[0] = face(1)),
    ];
    for (label, edit) in edits {
        let mut records = ring();
        edit(&mut records);
        assert_eq!(faces_of(&records), None, "{label}");
    }
}
