use super::super::disc22_disc12_disc20_disc1e_disc16_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x12, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1e, [7, 12, 14, 1, 1, 1]),
        record(14, 0x16, [7, 13, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(22, 0x0e, [102, 50, 1, 1, 1, 1]),
        record(23, 0x0e, [103, 999, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(51, 0x1c, [102, 1, 50, 1, 1, 1]),
        flo2(50, 0x14, [102, 51, 22, 1, 1, 1]),
        record(70, 0x1c, [103, 80, 1, 1, 1, 1]),
        record(71, 0x1c, [103, 81, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 1, 1, 1, 1]),
        flo4(60, 0x24, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_direct_forward_bridged_and_keyed_use_links() {
    let records = lattice();
    let bodies = disc22_disc12_disc20_disc1e_disc16_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc12-disc20-disc1e-disc16 face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&100) && body.refs.contains(&101) && body.refs.contains(&102));
    assert!(!body.refs.contains(&103));
}

#[test]
fn keyed_lattice_rejects_ambiguous_keyed_use_fallback() {
    let mut records = lattice();
    records.retain(|record| !matches!(record.attr, 23 | 70 | 71));
    records.push(flo4(61, 0x24, [102, 1, 1, 1, 1, 1]));

    assert!(disc22_disc12_disc20_disc1e_disc16_face_root_body(&index_records(&records)).is_empty());
}
