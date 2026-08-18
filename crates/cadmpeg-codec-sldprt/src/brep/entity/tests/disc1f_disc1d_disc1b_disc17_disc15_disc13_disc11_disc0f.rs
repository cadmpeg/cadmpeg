use super::super::disc1f_disc1d_disc1b_disc17_disc15_disc13_disc11_disc0f_direct_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1f, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1d, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1b, [7, 11, 13, 1, 1, 1]),
        record(13, 0x17, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x15, [7, 13, 15, 1, 1, 1]),
        flo2(15, 0x13, [7, 14, 16, 1, 1, 1]),
        flo2(16, 0x11, [7, 15, 17, 1, 1, 1]),
        flo2(17, 0x0f, [7, 16, 1, 1, 1, 1]),
        flo2(20, 0x1f, [3, 1, 21, 1, 1, 1]),
        flo2(21, 0x1d, [3, 20, 22, 1, 1, 1]),
        flo2(22, 0x1b, [3, 21, 23, 1, 1, 1]),
        record(23, 0x17, [3, 22, 24, 1, 1, 1]),
        flo2(24, 0x15, [3, 23, 25, 1, 1, 1]),
        flo2(25, 0x13, [3, 24, 26, 1, 1, 1]),
        flo2(26, 0x11, [3, 25, 27, 1, 1, 1]),
        flo2(27, 0x0f, [3, 26, 1, 1, 1, 1]),
        record(30, 0x04, [100, 31, 1, 1, 1, 1]),
        record(31, 0x19, [100, 32, 30, 1, 1, 1]),
        flo4(32, 0x21, [100, 1, 31, 1, 1, 1]),
        record(40, 0x04, [101, 41, 1, 1, 1, 1]),
        record(41, 0x19, [101, 42, 40, 1, 1, 1]),
        flo4(42, 0x21, [101, 1, 41, 1, 1, 1]),
    ]
}

#[test]
fn repeated_exact_chains_share_one_body_anchor() {
    let bodies = disc1f_disc1d_disc1b_disc17_disc15_disc13_disc11_disc0f_direct_face_root_body(
        &index_records(&lattice()),
    );
    let [body] = bodies.as_slice() else {
        panic!("one repeated-chain body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions.len(), 1);
    assert_eq!(body.regions[0].shells.len(), 1);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    assert!(body.refs.contains(&10) && body.refs.contains(&20));
    assert!(body.refs.contains(&30) && body.refs.contains(&40));
}

#[test]
fn repeated_exact_chains_reject_an_incomplete_duplicate() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 24)
        .expect("second chain record")
        .refs[1] = 99;

    assert!(
        disc1f_disc1d_disc1b_disc17_disc15_disc13_disc11_disc0f_direct_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}

#[test]
fn repeated_exact_chains_reject_a_nonreciprocal_face_link() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second companion")
        .refs[2] = 1;

    assert!(
        disc1f_disc1d_disc1b_disc17_disc15_disc13_disc11_disc0f_direct_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}
