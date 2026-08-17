use std::collections::HashMap;

use super::super::{
    disc1a_disc12_disc04_face_root_body, disc1a_disc14_disc04_face_root_body,
    disc1a_disc14_disc0c_face_root_body, BodyRecord, EntityRecord,
};
use super::{flo2, flo4, index_records, record};

fn chain_record(attr: u16, disc: u16, flo: u8, refs: [u16; 6]) -> EntityRecord {
    match flo {
        1 => record(attr, disc, refs),
        2 => flo2(attr, disc, refs),
        _ => panic!("unsupported chain flo"),
    }
}

fn lattice(
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
) -> Vec<EntityRecord> {
    let mut records = chain_shape
        .iter()
        .enumerate()
        .map(|(index, (disc, flo))| {
            let attr = 10 + index as u16;
            let previous = if index == 0 { 1 } else { attr - 1 };
            let next = if index + 1 == chain_shape.len() {
                1
            } else {
                attr + 1
            };
            chain_record(attr, *disc, *flo, [3, previous, next, 1, 1, 1])
        })
        .collect::<Vec<_>>();
    records.extend([
        record(20, canonical_disc, [1, 30, 1, 1, 1, 1]),
        record(21, canonical_disc, [1, 31, 1, 1, 1, 1]),
        record(30, companion_disc, [1, 40, 20, 1, 1, 1]),
        record(31, companion_disc, [1, 41, 21, 1, 1, 1]),
        flo4(40, 0x18, [1, 1, 30, 1, 1, 1]),
        flo4(41, 0x18, [1, 1, 31, 1, 1, 1]),
    ]);
    records
}

fn assert_body(
    records: &[EntityRecord],
    resolve: fn(&HashMap<u16, &EntityRecord>) -> Vec<BodyRecord>,
) {
    let bodies = resolve(&index_records(records));
    let [body] = bodies.as_slice() else {
        panic!("one keyed disc1a body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 11);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}

#[test]
fn disc14_disc04_chain_owns_disc0c_faces() {
    let records = lattice(
        &[
            (0x1a, 2),
            (0x16, 2),
            (0x14, 2),
            (0x10, 1),
            (0x0e, 2),
            (0x04, 2),
        ],
        0x0c,
        0x12,
    );
    assert_body(&records, disc1a_disc14_disc04_face_root_body);
}

#[test]
fn disc14_disc0c_chain_owns_disc04_faces() {
    let records = lattice(
        &[
            (0x1a, 2),
            (0x16, 2),
            (0x14, 2),
            (0x10, 1),
            (0x0e, 2),
            (0x0c, 2),
        ],
        0x04,
        0x12,
    );
    assert_body(&records, disc1a_disc14_disc0c_face_root_body);
}

#[test]
fn disc12_disc04_chain_owns_disc0c_faces() {
    let records = lattice(
        &[
            (0x1a, 2),
            (0x16, 2),
            (0x12, 1),
            (0x10, 2),
            (0x0e, 2),
            (0x04, 2),
        ],
        0x0c,
        0x14,
    );
    assert_body(&records, disc1a_disc12_disc04_face_root_body);
}

#[test]
fn keyed_chain_requires_reciprocal_links() {
    let records = lattice(
        &[
            (0x1a, 2),
            (0x16, 2),
            (0x14, 2),
            (0x10, 1),
            (0x0e, 2),
            (0x04, 2),
        ],
        0x0c,
        0x12,
    );
    let mut broken_companion = records.clone();
    broken_companion[8].refs[2] = 1;
    assert!(disc1a_disc14_disc04_face_root_body(&index_records(&broken_companion)).is_empty());

    let mut broken_terminal = records;
    broken_terminal[5].refs[2] = 0;
    assert!(disc1a_disc14_disc04_face_root_body(&index_records(&broken_terminal)).is_empty());
}
