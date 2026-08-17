use super::super::keyed_disc1a_disc18_disc14_disc12_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x14, [3, 11, 13, 1, 1, 1]),
        record(13, 0x12, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x04, [3, 13, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(30, 0x16, [100, 40, 20, 1, 1, 1]),
        record(31, 0x16, [101, 41, 21, 1, 1, 1]),
        flo4(40, 0x1c, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1c, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x1c, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_directly_linked_faces() {
    let records = lattice();
    let bodies = keyed_disc1a_disc18_disc14_disc12_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one keyed disc1a-disc18-disc14-disc12 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
    assert!(body.refs.contains(&42));
}

#[test]
fn keyed_lattice_rejects_broken_use_back_reference() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second use node")
        .refs[2] = 1;

    assert!(keyed_disc1a_disc18_disc14_disc12_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[0] = 102;

    assert!(keyed_disc1a_disc18_disc14_disc12_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_lattice_uses_face_key_when_companion_link_is_unusable() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first face")
        .refs[1] = 1;

    let bodies = keyed_disc1a_disc18_disc14_disc12_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}
