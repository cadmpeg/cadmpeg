use super::super::disc24_disc20_disc1e_disc1a_disc16_disc14_disc0e_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x24, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        record(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x16, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x0e, [3, 15, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(30, 0x22, [100, 40, 20, 1, 1, 1]),
        record(31, 0x22, [101, 41, 21, 1, 1, 1]),
        flo4(40, 0x26, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x26, [101, 1, 31, 1, 1, 1]),
        flo4(50, 0x26, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn lattice_owns_faces_and_ignores_extra_companion_and_use_nodes() {
    let bodies =
        disc24_disc20_disc1e_disc1a_disc16_disc14_disc0e_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc24-disc20-disc1e-disc1a-disc16-disc14-disc0e body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&40) && body.refs.contains(&41) && body.refs.contains(&50));
}

#[test]
fn lattice_accepts_forward_same_key_links() {
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

    assert_eq!(
        disc24_disc20_disc1e_disc1a_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .len(),
        1
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
        disc24_disc20_disc1e_disc1a_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn lattice_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("disc14 chain record")
        .refs[2] = 1;

    assert!(
        disc24_disc20_disc1e_disc1a_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}
