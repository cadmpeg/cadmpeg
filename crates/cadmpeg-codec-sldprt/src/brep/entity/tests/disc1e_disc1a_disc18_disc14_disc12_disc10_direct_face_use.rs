use super::super::disc1e_disc1a_disc18_disc14_disc12_disc10_direct_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x18, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x12, [7, 13, 15, 1, 1, 1]),
        record(15, 0x10, [7, 14, 1, 1, 1, 1]),
        record(20, 0x16, [100, 30, 1, 1, 1, 1]),
        record(21, 0x16, [101, 31, 1, 1, 1, 1]),
        record(22, 0x16, [102, 1, 1, 1, 1, 1]),
        flo4(30, 0x1c, [100, 1, 20, 1, 1, 1]),
        flo4(31, 0x1c, [101, 1, 21, 1, 1, 1]),
    ]
}

#[test]
fn direct_keyed_face_use_lattice_owns_selected_faces_and_ignores_unselected_faces() {
    let bodies =
        disc1e_disc1a_disc18_disc14_disc12_disc10_direct_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1a-disc18-disc14-disc12-disc10 direct face-use body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 15);
    assert!(body.refs.contains(&100) && body.refs.contains(&101));
    assert!(!body.regions[0].shells[0].refs.contains(&102));
}

#[test]
fn direct_keyed_face_use_lattice_rejects_a_missing_use_back_reference() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second use node")
        .refs[2] = 1;

    assert!(
        disc1e_disc1a_disc18_disc14_disc12_disc10_direct_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn direct_keyed_face_use_lattice_rejects_a_broken_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 13)
        .expect("disc14 chain record")
        .refs[1] = 99;

    assert!(
        disc1e_disc1a_disc18_disc14_disc12_disc10_direct_face_root_body(&index_records(&records))
            .is_empty()
    );
}
