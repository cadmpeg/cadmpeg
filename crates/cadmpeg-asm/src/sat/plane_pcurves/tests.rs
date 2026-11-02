// SPDX-License-Identifier: Apache-2.0

use super::{convert, face_bound_pcurves};
use crate::sab::{Record, Token};
use std::collections::BTreeSet;

fn record(index: i64, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index: usize::try_from(index).expect("test value fits"),
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn refs(fields: &[i64]) -> Vec<Token> {
    fields.iter().copied().map(Token::Ref).collect()
}

/// A ring of `count` coedges from `first`, owned by loop `owner`, each bound
/// to pcurve `pcurve + position`.
fn ring(first: i64, count: i64, owner: i64, pcurve: i64) -> Vec<Record> {
    (0..count)
        .map(|coedge| {
            let next = first + (coedge + 1) % count;
            let fields = [-1, -1, -1, next, -1, -1, -1, -1, owner, pcurve + coedge];
            record(first + coedge, "coedge", refs(&fields))
        })
        .collect()
}

#[test]
fn faces_sharing_one_loop_bind_its_pcurves_once_per_kind() {
    // Walking the ring once per face would take 2^24 steps.
    const FACES: i64 = 1 << 12;
    const COEDGES: i64 = 1 << 12;
    const PCURVES: i64 = 1 << 30;
    let (cone_face, plane, cone, shared) = (FACES, FACES + 1, FACES + 2, FACES + 3);
    let face = |index, surface| {
        record(
            index,
            "face",
            refs(&[-1, -1, -1, -1, shared, -1, -1, surface]),
        )
    };
    let mut records = (0..FACES)
        .map(|index| face(index, plane))
        .collect::<Vec<_>>();
    records.push(face(cone_face, cone));
    records.push(record(plane, "plane", Vec::new()));
    records.push(record(cone, "cone", Vec::new()));
    records.push(record(
        shared,
        "loop",
        refs(&[-1, -1, -1, -1, shared + 1, 0]),
    ));
    records.extend(ring(shared + 1, COEDGES, shared, PCURVES));
    let (on_plane, elsewhere) = face_bound_pcurves(&records);
    let expected = (PCURVES..PCURVES + COEDGES)
        .map(|pcurve| usize::try_from(pcurve).expect("test value fits"))
        .collect::<BTreeSet<_>>();
    assert_eq!(on_plane, expected);
    assert_eq!(elsewhere, expected);
}

#[test]
fn many_blocks_of_one_owner_convert_with_one_copy() {
    // Copying the owner's tokens once per block would clone 2^28 tokens.
    const BLOCKS: i64 = 1 << 12;
    let (face, plane, lp, first_coedge) = (0, 1, 2, 3);
    let first_pcurve = first_coedge + BLOCKS;
    let owner = first_pcurve + BLOCKS;
    let mut records = vec![
        record(face, "face", refs(&[-1, -1, -1, -1, lp, -1, -1, plane])),
        record(plane, "plane", Vec::new()),
        record(lp, "loop", refs(&[-1, -1, -1, -1, first_coedge, face])),
    ];
    records.extend(ring(first_coedge, BLOCKS, lp, first_pcurve));
    records.extend((0..BLOCKS).map(|block| {
        let tokens = vec![
            Token::SubtypeOpen,
            Token::Ident("ref".into()),
            Token::Long(block),
            Token::SubtypeClose,
        ];
        record(first_pcurve + block, "pcurve", tokens)
    }));
    let block = |block: i64| {
        let pole = f64::from(i32::try_from(block).expect("test value fits"));
        vec![
            Token::SubtypeOpen,
            Token::Ident("exp_par_cur".into()),
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Enum(0),
            Token::Long(2),
            Token::Double(0.0),
            Token::Long(1),
            Token::Double(1.0),
            Token::Long(1),
            Token::Double(pole),
            Token::Double(1.0),
            Token::Double(2.0),
            Token::Double(pole),
            Token::SubtypeClose,
        ]
    };
    records.push(record(
        owner,
        "intcurve",
        (0..BLOCKS).flat_map(block).collect(),
    ));
    let owner = usize::try_from(owner).expect("test value fits");
    let inch = 25.4;
    convert(&mut records, &[(0, inch), (owner + 1, 1.0)]).unwrap();
    let expected = (0..BLOCKS)
        .flat_map(|index| {
            let mut tokens = block(index);
            for coordinate in &mut tokens[10..14] {
                if let Token::Double(value) = coordinate {
                    *value = super::length_cm(*value, inch).unwrap();
                }
            }
            tokens
        })
        .collect::<Vec<_>>();
    assert_eq!(&records[owner].tokens[..], &expected[..]);
}

#[test]
fn pcurve_offset_conversion_leaves_dangling_and_other_record_references_for_topology() {
    for target in [4, 1 << 30] {
        let mut records = vec![
            record(0, "face", refs(&[-1, -1, -1, -1, 2, -1, -1, 1])),
            record(1, "plane", Vec::new()),
            record(2, "loop", refs(&[-1, -1, -1, -1, 3, 0])),
        ];
        records.extend(ring(3, 1, 2, target));
        records.push(record(
            4,
            "edge",
            vec![Token::Double(2.0), Token::Double(3.0)],
        ));
        convert(&mut records, &[(0, 25.4)]).expect("topology resolves the pcurve reference");
        assert_eq!(
            &*records[4].tokens,
            &[Token::Double(2.0), Token::Double(3.0)]
        );
    }
}
