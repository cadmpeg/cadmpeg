use super::super::disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        record(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 50, 21, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo2(50, 0x18, [101, 41, 31, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 50, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_direct_and_bridged_faces() {
    let bodies =
        disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc1e-disc1c-disc16-disc14-disc10-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
    assert!(body.refs.contains(&42));
}

#[test]
fn keyed_lattice_selects_by_key_when_face_link_is_not_reciprocal() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[2] = 1;

    let bodies =
        disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].refs.contains(&21) && bodies[0].refs.contains(&41));
}

#[test]
fn keyed_lattice_rejects_broken_use_back_reference() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second use node")
        .refs[2] = 1;

    assert!(
        disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(
        disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}
