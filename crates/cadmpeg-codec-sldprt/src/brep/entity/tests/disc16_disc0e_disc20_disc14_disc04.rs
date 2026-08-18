use super::super::disc16_disc0e_disc20_disc14_disc04_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x16, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x0e, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x1c, [100, 30, 1, 1, 1, 1]),
        record(21, 0x1c, [101, 31, 1, 1, 1, 1]),
        record(22, 0x1c, [102, 999, 1, 1, 1, 1]),
        record(23, 0x1c, [103, 998, 1, 1, 1, 1]),
        record(30, 0x1e, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1e, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1e, [102, 42, 1, 1, 1, 1]),
        record(33, 0x1e, [103, 43, 1, 1, 1, 1]),
        record(34, 0x1e, [103, 44, 1, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_direct_forward_and_unique_fallback_faces() {
    let records = lattice();
    let bodies = disc16_disc0e_disc20_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc16-disc0e-disc20-disc14-disc04 face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&100) && body.refs.contains(&101) && body.refs.contains(&102));
    assert!(!body.refs.contains(&103));
}

#[test]
fn keyed_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("forward companion")
        .refs[0] = 999;

    let bodies = disc16_disc0e_disc20_disc14_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one body with the mismatched face excluded");
    };
    assert!(body.refs.contains(&100) && body.refs.contains(&102));
    assert!(!body.refs.contains(&101));
}
