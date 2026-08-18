use super::super::disc28_disc26_disc24_disc1a_disc18_disc16_disc06_face_root_body;
use super::{flo2, flo4, index_records, record};

fn flo3(attr: u16, disc: u16, refs: [u16; 6]) -> super::super::EntityRecord {
    let mut record = record(attr, disc, refs);
    record.flags = 3;
    record
}

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x28, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x26, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x24, [7, 11, 13, 1, 1, 1]),
        record(13, 0x1a, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x18, [7, 13, 15, 1, 1, 1]),
        flo2(15, 0x16, [7, 14, 16, 1, 1, 1]),
        flo2(16, 0x06, [7, 15, 1, 1, 1, 1]),
        record(20, 0x14, [100, 30, 1, 1, 1, 1]),
        record(21, 0x14, [101, 31, 1, 1, 1, 1]),
        record(22, 0x14, [102, 90, 1, 1, 1, 1]),
        record(23, 0x14, [103, 99, 1, 1, 1, 1]),
        record(30, 0x22, [100, 40, 20, 1, 1, 1]),
        record(31, 0x22, [101, 41, 1, 1, 1, 1]),
        record(32, 0x22, [102, 42, 1, 1, 1, 1]),
        flo3(90, 0x1c, [102, 91, 1, 1, 1, 1]),
        flo4(40, 0x2a, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x2a, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x2a, [102, 1, 32, 1, 1, 1]),
        flo4(60, 0x2a, [999, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn lattice_owns_direct_forward_and_fallback_faces() {
    let bodies =
        disc28_disc26_disc24_disc1a_disc18_disc16_disc06_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc28-disc26-disc24-disc1a-disc18-disc16-disc06 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&100));
    assert!(body.refs.contains(&101));
    assert!(body.refs.contains(&102));
    assert!(!body.refs.contains(&103));
    assert!(body.regions[0].shells[0].refs.contains(&100));
    assert!(body.regions[0].shells[0].refs.contains(&101));
    assert!(body.regions[0].shells[0].refs.contains(&102));
    assert!(!body.regions[0].shells[0].refs.contains(&103));
}

#[test]
fn lattice_rejects_a_forward_face_with_no_use() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[1] = 99;

    assert!(
        disc28_disc26_disc24_disc1a_disc18_disc16_disc06_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn lattice_rejects_a_fallback_use_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 42)
        .expect("fallback use")
        .refs[0] = 999;

    assert!(
        disc28_disc26_disc24_disc1a_disc18_disc16_disc06_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn lattice_rejects_a_broken_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("disc16 chain record")
        .refs[2] = 1;

    assert!(
        disc28_disc26_disc24_disc1a_disc18_disc16_disc06_face_root_body(&index_records(&records))
            .is_empty()
    );
}
