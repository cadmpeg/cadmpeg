use super::super::{
    disc1e_disc1c_disc1a_disc14_disc12_face_root_body,
    disc1e_disc1c_disc1a_disc16_disc12_face_root_body,
};
use super::{flo2, flo4, index_records, record};

fn lattice(shell_disc: u16) -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1c, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [7, 11, 13, 1, 1, 1]),
        record(13, shell_disc, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x12, [7, 13, 15, 1, 1, 1]),
        flo2(15, 0x04, [7, 14, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 99, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 20, 1, 1, 1]),
        record(31, 0x18, [101, 41, 1, 1, 1, 1]),
        record(32, 0x18, [999, 1, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 31, 1, 1, 1]),
    ]
}

#[test]
fn disc14_chain_accepts_reciprocal_and_keyed_fallback_face_links() {
    let bodies = disc1e_disc1c_disc1a_disc14_disc12_face_root_body(&index_records(&lattice(0x14)));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc1a-disc14-disc12 body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 13));
    assert!(body.refs.contains(&32));
}

#[test]
fn disc16_chain_accepts_the_same_face_lattice() {
    let bodies = disc1e_disc1c_disc1a_disc16_disc12_face_root_body(&index_records(&lattice(0x16)));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc1a-disc16-disc12 body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 13));
    assert!(body.refs.contains(&32));
}

#[test]
fn keyed_face_lattice_rejects_an_ambiguous_companion_fallback() {
    let mut records = lattice(0x14);
    records.push(record(33, 0x18, [101, 42, 1, 1, 1, 1]));

    assert!(disc1e_disc1c_disc1a_disc14_disc12_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_face_lattice_rejects_a_broken_companion_use_backlink() {
    let mut records = lattice(0x16);
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("fallback companion use")
        .refs[2] = 1;

    assert!(disc1e_disc1c_disc1a_disc16_disc12_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_face_lattice_rejects_the_sibling_shell_chain() {
    let records = lattice(0x16);
    assert!(disc1e_disc1c_disc1a_disc14_disc12_face_root_body(&index_records(&records)).is_empty());

    let records = lattice(0x14);
    assert!(disc1e_disc1c_disc1a_disc16_disc12_face_root_body(&index_records(&records)).is_empty());
}
