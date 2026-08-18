use super::super::disc24_disc22_disc20_disc1c_disc16_disc14_disc0e_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x24, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x22, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [3, 12, 14, 1, 1, 1]),
        record(14, 0x16, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x0e, [3, 15, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(30, 0x1e, [100, 40, 20, 1, 1, 1]),
        flo4(40, 0x26, [100, 1, 30, 1, 1, 1]),
        record(21, 0x04, [101, 50, 1, 1, 1, 1]),
        record(50, 0x10, [101, 60, 21, 1, 1, 1]),
        record(60, 0x1e, [101, 70, 50, 1, 1, 1]),
        flo4(70, 0x26, [101, 1, 60, 1, 1, 1]),
        record(61, 0x1e, [101, 71, 1, 1, 1, 1]),
        record(22, 0x04, [102, 80, 1, 1, 1, 1]),
        flo2(80, 0x1a, [102, 90, 22, 1, 1, 1]),
        record(90, 0x1e, [102, 100, 80, 1, 1, 1]),
        flo4(100, 0x26, [102, 1, 90, 1, 1, 1]),
        record(91, 0x1e, [102, 101, 1, 1, 1, 1]),
        record(23, 0x04, [103, 999, 1, 1, 1, 1]),
        record(110, 0x1e, [103, 120, 1, 1, 1, 1]),
        flo4(120, 0x26, [103, 1, 110, 1, 1, 1]),
        record(111, 0x1e, [104, 121, 1, 1, 1, 1]),
        flo4(130, 0x26, [105, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn mixed_face_links_assign_direct_bridged_and_keyed_faces() {
    let records = lattice();
    let bodies =
        disc24_disc22_disc20_disc1c_disc16_disc14_disc0e_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc24-disc22-disc20-disc1c-disc16-disc14-disc0e body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    for attr in [
        20, 21, 22, 23, 30, 50, 60, 80, 90, 110, 40, 70, 100, 120, 130,
    ] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
    }
}

#[test]
fn mixed_face_links_reject_a_broken_bridge_with_ambiguous_fallback() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 90)
        .expect("0x1a bridge companion")
        .refs[2] = 1;

    assert!(
        disc24_disc22_disc20_disc1c_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn mixed_face_links_reject_a_bridge_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 80)
        .expect("0x1a bridge")
        .refs[0] = 999;

    assert!(
        disc24_disc22_disc20_disc1c_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn mixed_face_links_reject_a_missing_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 70)
        .collect::<Vec<_>>();

    assert!(
        disc24_disc22_disc20_disc1c_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn mixed_face_links_reject_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("disc14 chain record")
        .refs[2] = 1;

    assert!(
        disc24_disc22_disc20_disc1c_disc16_disc14_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}
