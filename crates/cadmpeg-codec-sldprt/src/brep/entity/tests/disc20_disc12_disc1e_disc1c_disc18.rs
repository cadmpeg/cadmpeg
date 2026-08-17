use super::super::disc20_disc12_disc1e_disc1c_disc18_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x12, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [3, 12, 14, 1, 1, 1]),
        record(14, 0x18, [3, 13, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 41, 21, 1, 1, 1]),
        record(32, 0x1a, [102, 51, 1, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 31, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_directly_linked_faces() {
    let records = lattice();
    let bodies = disc20_disc12_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc12-disc1e-disc1c-disc18 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&32));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn keyed_lattice_selects_by_key_when_face_companion_back_reference_is_broken() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 21)
        .expect("second canonical face")
        .refs[1] = 999;
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[2] = 1;

    let bodies = disc20_disc12_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].refs.contains(&21) && bodies[0].refs.contains(&41));
}

#[test]
fn keyed_lattice_accepts_forward_keyed_links_when_reverse_links_are_stale() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[2] = 1;
    records
        .iter_mut()
        .find(|record| record.attr == 40)
        .expect("first use node")
        .refs[2] = 1;

    let bodies = disc20_disc12_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn keyed_lattice_rejects_a_broken_forward_use_link() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[1] = 999;

    assert!(disc20_disc12_disc1e_disc1c_disc18_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc20_disc12_disc1e_disc1c_disc18_face_root_body(&index_records(&records)).is_empty());
}
