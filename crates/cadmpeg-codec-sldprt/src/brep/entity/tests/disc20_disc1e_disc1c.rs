use super::super::{
    disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc04_face_root_body,
    disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc0e_face_root_body,
};
use super::{flo2, flo4, index_records, record};

fn lattice(terminal_disc: u16, canonical_disc: u16) -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
        record(14, 0x16, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x12, [3, 15, 17, 1, 1, 1]),
        flo2(17, terminal_disc, [3, 16, 1, 1, 1, 1]),
        record(20, canonical_disc, [100, 30, 1, 1, 1, 1]),
        record(21, canonical_disc, [101, 31, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 41, 21, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn terminal_disc0e_lattice_owns_disc04_faces() {
    let records = lattice(0x000e, 0x0004);
    let bodies = disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc0e_face_root_body(
        &index_records(&records),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc1e-disc1c-disc18-disc16-disc14-disc12-disc0e body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
    assert!(body.refs.contains(&42));
}

#[test]
fn terminal_disc04_lattice_owns_disc0e_faces() {
    let records = lattice(0x0004, 0x000e);
    let bodies = disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc04_face_root_body(
        &index_records(&records),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc1e-disc1c-disc18-disc16-disc14-disc12-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}

#[test]
fn lattice_rejects_broken_use_back_reference() {
    let mut records = lattice(0x000e, 0x0004);
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second use node")
        .refs[2] = 1;
    assert!(
        disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc0e_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}

#[test]
fn lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice(0x000e, 0x0004);
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[0] = 102;
    assert!(
        disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc0e_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}
