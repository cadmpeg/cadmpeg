use super::super::disc21_disc1f_disc1b_disc19_disc15_disc13_disc11_disc04_face_root_body;
use super::{flo2, flo4, index_records, record};

fn flo5(attr: u16, disc: u16, refs: [u16; 6]) -> super::super::EntityRecord {
    let mut out = record(attr, disc, refs);
    out.flags = 5;
    out
}

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x21, [3, 1, 11, 1, 1, 1]),
        flo5(11, 0x1f, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1b, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x19, [3, 12, 14, 1, 1, 1]),
        record(14, 0x15, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x13, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x11, [3, 15, 17, 1, 1, 1]),
        flo2(17, 0x04, [3, 16, 1, 1, 1, 1]),
        record(20, 0x0f, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0f, [101, 31, 1, 1, 1, 1]),
        record(30, 0x17, [100, 40, 20, 1, 1, 1]),
        record(31, 0x17, [101, 41, 21, 1, 1, 1]),
        record(32, 0x17, [102, 42, 1, 1, 1, 1]),
        flo4(40, 0x1d, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1d, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x1d, [102, 1, 32, 1, 1, 1]),
    ]
}

#[test]
fn lattice_owns_direct_faces_and_ignores_unselected_records() {
    let bodies = disc21_disc1f_disc1b_disc19_disc15_disc13_disc11_disc04_face_root_body(
        &index_records(&lattice()),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc21-disc1f-disc1b-disc19-disc15-disc13-disc11-disc04 body");
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
        disc21_disc1f_disc1b_disc19_disc15_disc13_disc11_disc04_face_root_body(&index_records(
            &records
        ))
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
        disc21_disc1f_disc1b_disc19_disc15_disc13_disc11_disc04_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}

#[test]
fn lattice_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 11)
        .expect("flo5 chain record")
        .refs[2] = 1;

    assert!(
        disc21_disc1f_disc1b_disc19_disc15_disc13_disc11_disc04_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}
