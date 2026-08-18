use super::super::disc1e_disc1c_disc1a_disc16_disc10_disc0e_disc04_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
        record(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x12, [100, 30, 1, 1, 1, 1]),
        record(21, 0x12, [101, 31, 1, 1, 1, 1]),
        record(22, 0x12, [102, 50, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 1, 1, 1, 1]),
        record(31, 0x18, [101, 41, 21, 1, 1, 1]),
        record(32, 0x18, [103, 42, 1, 1, 1, 1]),
        record(50, 0x14, [102, 1, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x20, [103, 1, 32, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_selects_compatible_faces_and_owns_the_disc1a_shell() {
    let bodies =
        disc1e_disc1c_disc1a_disc16_disc10_disc0e_disc04_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc1a-disc16-disc10-disc0e-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(!body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn keyed_lattice_rejects_a_broken_chain_predecessor() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 13)
        .expect("disc16 chain record")
        .refs[1] = 99;

    assert!(
        disc1e_disc1c_disc1a_disc16_disc10_disc0e_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_selected_use_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 40)
        .expect("first use node")
        .refs[0] = 101;

    assert!(
        disc1e_disc1c_disc1a_disc16_disc10_disc0e_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}
