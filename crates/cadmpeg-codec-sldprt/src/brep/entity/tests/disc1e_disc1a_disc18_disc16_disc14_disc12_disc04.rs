use super::super::disc1e_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
        record(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 50, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 51, 1, 1, 1, 1]),
        record(50, 0x10, [100, 60, 20, 1, 1, 1]),
        record(51, 0x10, [101, 61, 21, 1, 1, 1]),
        record(60, 0x1c, [100, 70, 50, 1, 1, 1]),
        record(61, 0x1c, [101, 71, 51, 1, 1, 1]),
        flo4(70, 0x20, [100, 1, 60, 1, 1, 1]),
        flo4(71, 0x20, [101, 1, 61, 1, 1, 1]),
        flo4(80, 0x20, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn bridged_lattice_owns_faces_and_ignores_extra_use_nodes() {
    let bodies =
        disc1e_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1a-disc18-disc16-disc14-disc12-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&50) && body.refs.contains(&51));
    assert!(body.refs.contains(&60) && body.refs.contains(&61) && body.refs.contains(&80));
}

#[test]
fn bridged_lattice_rejects_a_broken_face_bridge() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 51)
        .expect("second face bridge")
        .refs[2] = 1;

    assert!(
        disc1e_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn bridged_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 61)
        .expect("second companion")
        .refs[0] = 102;

    assert!(
        disc1e_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn bridged_lattice_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("disc12 chain record")
        .refs[2] = 1;

    assert!(
        disc1e_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}
