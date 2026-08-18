use super::super::disc1c_disc1a_disc16_disc14_disc12_disc10_tail_reciprocal_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x10, [3, 15, 17, 1, 1, 1]),
        flo2(17, 0x10, [3, 16, 18, 1, 1, 1]),
        flo2(18, 0x10, [3, 17, 19, 1, 1, 1]),
        flo2(19, 0x10, [3, 18, 20, 1, 1, 1]),
        flo2(20, 0x10, [3, 19, 21, 1, 1, 1]),
        flo2(21, 0x10, [3, 20, 1, 1, 1, 1]),
        record(30, 0x0e, [100, 40, 1, 1, 1, 1]),
        record(31, 0x0e, [101, 41, 1, 1, 1, 1]),
        record(40, 0x18, [100, 50, 30, 1, 1, 1]),
        record(41, 0x18, [101, 51, 31, 1, 1, 1]),
        record(42, 0x18, [999, 1, 1, 1, 1, 1]),
        flo4(50, 0x1e, [100, 1, 40, 1, 1, 1]),
        flo4(51, 0x1e, [101, 1, 41, 1, 1, 1]),
        flo4(52, 0x1e, [999, 1, 42, 1, 1, 1]),
    ]
}

#[test]
fn reciprocal_repeated_tail_lattice_accepts_extra_companion_and_use_records() {
    let bodies = disc1c_disc1a_disc16_disc14_disc12_disc10_tail_reciprocal_face_root_body(
        &index_records(&lattice()),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc1a-disc16-disc14-disc12-disc10 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    for attr in [30, 31, 40, 41, 42, 50, 51, 52] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
        assert!(
            body.regions[0].shells[0].refs.contains(&attr),
            "missing shell reference {attr}"
        );
    }
}

#[test]
fn reciprocal_repeated_tail_lattice_rejects_a_broken_companion_backlink() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second companion")
        .refs[2] = 1;

    assert!(
        disc1c_disc1a_disc16_disc14_disc12_disc10_tail_reciprocal_face_root_body(&index_records(
            &records
        ),)
        .is_empty()
    );
}

#[test]
fn reciprocal_repeated_tail_lattice_rejects_a_broken_use_backlink() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 51)
        .expect("second use node")
        .refs[2] = 1;

    assert!(
        disc1c_disc1a_disc16_disc14_disc12_disc10_tail_reciprocal_face_root_body(&index_records(
            &records
        ),)
        .is_empty()
    );
}

#[test]
fn reciprocal_repeated_tail_lattice_rejects_a_broken_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 17)
        .expect("repeated disc10 chain record")
        .refs[2] = 1;

    assert!(
        disc1c_disc1a_disc16_disc14_disc12_disc10_tail_reciprocal_face_root_body(&index_records(
            &records
        ),)
        .is_empty()
    );
}
