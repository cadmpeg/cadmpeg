use super::super::disc22_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc06_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        record(14, 0x18, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x16, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x06, [3, 15, 1, 1, 1, 1]),
        record(20, 0x14, [100, 30, 1, 1, 1, 1]),
        record(21, 0x14, [101, 31, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 21, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 31, 1, 1, 1]),
        flo4(60, 0x24, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn lattice_owns_direct_faces_and_ignores_unselected_records() {
    let bodies = disc22_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc06_face_root_body(
        &index_records(&lattice()),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc20-disc1e-disc1a-disc18-disc16-disc14-disc12-disc06 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn lattice_rejects_a_broken_companion_back_reference() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[2] = 1;

    assert!(
        disc22_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc06_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}

#[test]
fn lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(
        disc22_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc06_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}

#[test]
fn lattice_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("disc16 chain record")
        .refs[2] = 1;

    assert!(
        disc22_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc06_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}
