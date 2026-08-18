use super::super::disc22_disc20_disc1a_disc18_disc10_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x10, [3, 13, 1, 1, 1, 1]),
        record(20, 0x12, [100, 30, 1, 1, 1, 1]),
        record(21, 0x12, [101, 31, 1, 1, 1, 1]),
        record(22, 0x12, [102, 50, 1, 1, 1, 1]),
        record(23, 0x12, [103, 999, 1, 1, 1, 1]),
        record(24, 0x12, [104, 999, 1, 1, 1, 1]),
        record(30, 0x1e, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1e, [101, 41, 21, 1, 1, 1]),
        record(32, 0x1e, [103, 42, 1, 1, 1, 1]),
        record(33, 0x1e, [105, 43, 1, 1, 1, 1]),
        flo3(50, 0x1c, [102, 51, 22, 1, 1, 1]),
        record(51, 0x1e, [102, 60, 50, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x24, [103, 1, 32, 1, 1, 1]),
        flo4(43, 0x24, [105, 1, 33, 1, 1, 1]),
        flo4(60, 0x24, [102, 1, 51, 1, 1, 1]),
    ]
}

fn flo3(attr: u16, disc: u16, refs: [u16; 6]) -> super::super::EntityRecord {
    let mut out = record(attr, disc, refs);
    out.flags = 3;
    out
}

#[test]
fn keyed_lattice_selects_direct_bridged_and_same_key_faces() {
    let bodies = disc22_disc20_disc1a_disc18_disc10_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc20-disc1a-disc18-disc10 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&22) && body.refs.contains(&23));
    assert!(!body.refs.contains(&24));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&50) && body.refs.contains(&51));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn keyed_lattice_rejects_a_missing_selected_use() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 41)
        .collect::<Vec<_>>();

    assert!(disc22_disc20_disc1a_disc18_disc10_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_lattice_rejects_a_broken_chain_predecessor() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 13)
        .expect("disc18 chain record")
        .refs[1] = 99;

    assert!(disc22_disc20_disc1a_disc18_disc10_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn keyed_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc22_disc20_disc1a_disc18_disc10_face_root_body(&index_records(&records)).is_empty());
}
