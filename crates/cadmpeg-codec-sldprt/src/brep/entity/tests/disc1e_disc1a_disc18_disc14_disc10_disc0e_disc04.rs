use super::super::disc1e_disc1a_disc18_disc14_disc10_disc0e_disc04_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x18, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x10, [7, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [7, 14, 16, 1, 1, 1]),
        record(16, 0x04, [7, 15, 1, 1, 1, 1]),
        record(20, 0x16, [100, 30, 1, 1, 1, 1]),
        record(21, 0x16, [101, 31, 1, 1, 1, 1]),
        record(22, 0x16, [102, 999, 1, 1, 1, 1]),
        record(23, 0x16, [103, 999, 1, 1, 1, 1]),
        record(30, 0x12, [100, 40, 20, 1, 1, 1]),
        record(31, 0x12, [101, 41, 1, 1, 1, 1]),
        record(32, 0x12, [102, 999, 1, 1, 1, 1]),
        record(33, 0x12, [103, 43, 1, 1, 1, 1]),
        record(34, 0x12, [103, 44, 1, 1, 1, 1]),
        flo4(40, 0x1c, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1c, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x1c, [102, 1, 1, 1, 1, 1]),
        flo4(43, 0x1c, [103, 1, 1, 1, 1, 1]),
        flo4(44, 0x1c, [103, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_direct_forward_and_unique_fallback_faces() {
    let bodies =
        disc1e_disc1a_disc18_disc14_disc10_disc0e_disc04_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1a-disc18-disc14-disc10-disc0e-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(!body.refs.contains(&23));
    assert!(body.refs.contains(&100) && body.refs.contains(&101) && body.refs.contains(&102));
}

#[test]
fn keyed_lattice_accepts_unique_fallback_after_ambiguous_candidate_is_removed() {
    let mut records = lattice();
    records.retain(|record| record.attr != 34);

    let bodies =
        disc1e_disc1a_disc18_disc14_disc10_disc0e_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one body with the uniquely resolved fallback face");
    };
    assert!(body.refs.contains(&23));
}

#[test]
fn keyed_lattice_rejects_a_broken_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 14)
        .expect("disc10 chain record")
        .refs[1] = 99;

    assert!(
        disc1e_disc1a_disc18_disc14_disc10_disc0e_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}
