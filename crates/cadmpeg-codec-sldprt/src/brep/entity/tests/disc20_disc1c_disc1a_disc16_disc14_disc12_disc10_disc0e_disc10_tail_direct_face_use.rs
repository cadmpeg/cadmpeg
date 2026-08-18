use super::super::disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc0e_disc10_tail_direct_face_use_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
        record(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x10, [3, 15, 17, 1, 1, 1]),
        flo2(17, 0x0e, [3, 16, 18, 1, 1, 1]),
        flo2(18, 0x10, [3, 17, 19, 1, 1, 1]),
        flo2(19, 0x10, [3, 18, 20, 1, 1, 1]),
        flo2(20, 0x10, [3, 19, 21, 1, 1, 1]),
        flo2(21, 0x10, [3, 20, 22, 1, 1, 1]),
        flo2(22, 0x10, [3, 21, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 1, 1, 1, 1]),
        record(31, 0x18, [101, 41, 1, 1, 1, 1]),
        flo4(40, 0x1e, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1e, [101, 1, 31, 1, 1, 1]),
    ]
}

#[test]
fn repeated_tail_direct_face_use_lattice_owns_faces() {
    let bodies =
        disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc0e_disc10_tail_direct_face_use_root_body(
            &index_records(&lattice()),
        );
    let [body] = bodies.as_slice() else {
        panic!("one repeated-tail direct face-use body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    for attr in [30, 31, 40, 41, 100, 101] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
        assert!(
            body.regions[0].shells[0].refs.contains(&attr),
            "missing shell reference {attr}"
        );
    }
}

#[test]
fn repeated_tail_direct_face_use_lattice_rejects_a_nonreciprocal_use() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second use node")
        .refs[2] = 30;

    assert!(
        disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc0e_disc10_tail_direct_face_use_root_body(
            &index_records(&records),
        )
        .is_empty()
    );
}

#[test]
fn repeated_tail_direct_face_use_lattice_rejects_a_mismatched_use_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second use node")
        .refs[0] = 102;

    assert!(
        disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc0e_disc10_tail_direct_face_use_root_body(
            &index_records(&records),
        )
        .is_empty()
    );
}

#[test]
fn repeated_tail_direct_face_use_lattice_rejects_a_broken_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("repeated disc10 chain record")
        .refs[2] = 1;

    assert!(
        disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc0e_disc10_tail_direct_face_use_root_body(
            &index_records(&records),
        )
        .is_empty()
    );
}
