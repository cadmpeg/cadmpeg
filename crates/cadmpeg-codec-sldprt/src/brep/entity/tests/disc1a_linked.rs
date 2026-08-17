use std::collections::HashMap;

use super::super::{
    disc16_disc14_disc04_face_root_body, disc18_disc14_disc12_disc04_face_root_body,
    disc1a_disc12_disc04_face_root_body, disc1a_disc14_disc04_face_root_body,
    disc1a_disc14_disc0c_face_root_body, disc1a_disc14_disc12_face_root_body,
    disc1a_disc18_disc14_disc04_face_root_body, disc1c_disc1a_disc18_disc14_disc04_face_root_body,
    disc1e_disc18_disc16_disc14_disc04_face_root_body,
    disc1e_disc1a_disc18_disc14_disc04_face_root_body, disc1e_disc1c_disc14_disc0e_face_root_body,
    disc1e_disc1c_disc16_disc14_disc0e_face_root_body, disc1e_disc1c_disc16_disc14_face_root_body,
    disc20_disc1a_disc14_disc04_face_root_body, BodyRecord, EntityRecord,
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
    lattice_with_use(chain_shape, canonical_disc, companion_disc, 0x18)
}

fn lattice_with_use(
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
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
        flo4(40, use_disc, [1, 1, 30, 1, 1, 1]),
        flo4(41, use_disc, [1, 1, 31, 1, 1, 1]),
    ]);
    records
}

fn assert_body(
    records: &[EntityRecord],
    resolve: fn(&HashMap<u16, &EntityRecord>) -> Vec<BodyRecord>,
) {
    let bodies = resolve(&index_records(records));
    let [body] = bodies.as_slice() else {
        panic!("one keyed body");
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
fn disc14_disc12_chain_owns_disc0e_faces() {
    let records = lattice_with_use(
        &[(0x1a, 2), (0x16, 2), (0x12, 2), (0x10, 1), (0x04, 2)],
        0x0e,
        0x14,
        0x18,
    );
    assert_body(&records, disc1a_disc14_disc12_face_root_body);
}

#[test]
fn disc1a_disc18_disc14_disc04_chain_owns_disc0e_faces() {
    let records = lattice_with_use(
        &[
            (0x1a, 2),
            (0x18, 2),
            (0x14, 1),
            (0x12, 2),
            (0x10, 2),
            (0x04, 2),
        ],
        0x0e,
        0x16,
        0x1c,
    );
    let bodies = disc1a_disc18_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1a-disc18-disc14-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 11);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}

#[test]
fn disc16_disc14_disc04_chain_allows_auxiliary_use_nodes() {
    let mut records = lattice_with_use(
        &[(0x16, 2), (0x14, 2), (0x12, 2), (0x10, 1), (0x04, 2)],
        0x0e,
        0x18,
        0x1a,
    );
    records.push(flo4(50, 0x1a, [1; 6]));
    assert_body(&records, disc16_disc14_disc04_face_root_body);

    records[8].refs[2] = 1;
    assert!(disc16_disc14_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc18_disc14_disc12_disc04_chain_owns_disc0c_faces() {
    let records = lattice_with_use(
        &[
            (0x18, 2),
            (0x14, 2),
            (0x12, 2),
            (0x10, 1),
            (0x0e, 2),
            (0x04, 2),
        ],
        0x0c,
        0x16,
        0x1a,
    );
    assert_body(&records, disc18_disc14_disc12_disc04_face_root_body);
}

#[test]
fn disc20_disc1a_disc14_disc04_chain_owns_disc0e_faces() {
    let mut records = lattice_with_use(
        &[
            (0x20, 2),
            (0x1c, 2),
            (0x1a, 2),
            (0x14, 1),
            (0x12, 2),
            (0x04, 2),
        ],
        0x0e,
        0x10,
        0x1e,
    );
    records[8].refs[1] = 60;
    records[9].refs[1] = 51;
    records[10].refs[2] = 50;
    records[11].refs[2] = 51;
    records.extend([
        record(50, 0x18, [1, 40, 60, 1, 1, 1]),
        record(51, 0x18, [1, 41, 31, 1, 1, 1]),
        flo2(60, 0x16, [1, 50, 30, 1, 1, 1]),
    ]);
    let bodies = disc20_disc1a_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc1a-disc14-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}

#[test]
fn disc1e_disc1c_disc16_disc14_chain_owns_disc0e_faces() {
    let records = lattice_with_use(
        &[
            (0x1e, 2),
            (0x1c, 2),
            (0x1a, 2),
            (0x16, 2),
            (0x14, 1),
            (0x12, 2),
            (0x10, 2),
        ],
        0x0e,
        0x18,
        0x20,
    );
    let bodies = disc1e_disc1c_disc16_disc14_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc16-disc14 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}

#[test]
fn disc1e_disc1c_disc16_disc14_disc0e_chain_owns_disc04_faces() {
    let records = lattice_with_use(
        &[
            (0x1e, 2),
            (0x1c, 2),
            (0x1a, 2),
            (0x16, 2),
            (0x14, 1),
            (0x12, 2),
            (0x10, 2),
            (0x0e, 2),
        ],
        0x04,
        0x18,
        0x20,
    );
    let bodies = disc1e_disc1c_disc16_disc14_disc0e_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc16-disc14-disc0e body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}

#[test]
fn disc1e_disc1c_disc14_disc0e_chain_owns_disc04_faces() {
    let mut records = lattice_with_use(
        &[
            (0x1e, 2),
            (0x1c, 2),
            (0x1a, 2),
            (0x14, 1),
            (0x12, 2),
            (0x10, 2),
            (0x0e, 2),
        ],
        0x04,
        0x18,
        0x20,
    );
    records[7].refs[1] = 60;
    records[9].refs[2] = 60;
    records.push(flo2(60, 0x16, [1, 30, 20, 1, 1, 1]));
    let bodies = disc1e_disc1c_disc14_disc0e_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc14-disc0e body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&60));

    let mut broken_bridge = records;
    broken_bridge[13].refs[2] = 21;
    assert!(disc1e_disc1c_disc14_disc0e_face_root_body(&index_records(&broken_bridge)).is_empty());
}

#[test]
fn disc1e_disc1a_disc18_disc14_disc04_chain_owns_disc0e_faces() {
    let mut records = lattice_with_use(
        &[
            (0x1e, 2),
            (0x1a, 2),
            (0x18, 2),
            (0x14, 1),
            (0x12, 2),
            (0x10, 2),
            (0x04, 2),
        ],
        0x0e,
        0x1c,
        0x20,
    );
    records[7].refs[1] = 60;
    records[9].refs[2] = 60;
    records.extend([flo2(60, 0x16, [1, 30, 20, 1, 1, 1]), flo4(50, 0x20, [1; 6])]);
    let bodies = disc1e_disc1a_disc18_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1a-disc18-disc14-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&50));
}

#[test]
fn disc1e_disc18_disc16_disc14_disc04_chain_owns_disc0e_faces() {
    let mut records = lattice_with_use(
        &[
            (0x1e, 2),
            (0x18, 2),
            (0x16, 2),
            (0x14, 1),
            (0x12, 2),
            (0x10, 2),
            (0x04, 2),
        ],
        0x0e,
        0x1c,
        0x20,
    );
    records[7].refs[1] = 60;
    records[9].refs[2] = 60;
    records.push(flo2(60, 0x1a, [1, 30, 20, 1, 1, 1]));
    let bodies = disc1e_disc18_disc16_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc18-disc16-disc14-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&60));
}

#[test]
fn disc1c_disc1a_disc18_disc14_disc04_chain_owns_disc0e_faces() {
    let mut records = lattice_with_use(
        &[
            (0x1c, 2),
            (0x1a, 2),
            (0x18, 2),
            (0x14, 1),
            (0x10, 2),
            (0x04, 2),
        ],
        0x0e,
        0x16,
        0x1e,
    );
    records[6].refs[1] = 60;
    records[8].refs[2] = 60;
    records.push(flo2(60, 0x12, [1, 30, 20, 1, 1, 1]));
    let bodies = disc1c_disc1a_disc18_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc1a-disc18-disc14-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&60));

    let mut broken_bridge = records;
    broken_bridge[12].refs[2] = 21;
    assert!(
        disc1c_disc1a_disc18_disc14_disc04_face_root_body(&index_records(&broken_bridge))
            .is_empty()
    );
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
