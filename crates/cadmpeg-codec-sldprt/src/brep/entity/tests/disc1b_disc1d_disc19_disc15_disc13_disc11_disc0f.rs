use super::super::disc1b_disc1d_disc19_disc15_disc13_disc11_disc0f_direct_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1b, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1d, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x19, [7, 11, 13, 1, 1, 1]),
        record(13, 0x15, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x13, [7, 13, 15, 1, 1, 1]),
        flo2(15, 0x11, [7, 14, 16, 1, 1, 1]),
        flo2(16, 0x0f, [7, 15, 1, 1, 1, 1]),
        record(20, 0x17, [100, 30, 1, 1, 1, 1]),
        record(21, 0x17, [101, 31, 1, 1, 1, 1]),
        flo4(30, 0x1f, [100, 1, 20, 1, 1, 1]),
        flo4(31, 0x1f, [101, 1, 21, 1, 1, 1]),
    ]
}

#[test]
fn direct_face_use_lattice_owns_faces() {
    let bodies = disc1b_disc1d_disc19_disc15_disc13_disc11_disc0f_direct_face_root_body(
        &index_records(&lattice()),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc1b-disc1d-disc19-disc15-disc13-disc11-disc0f body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&100) && body.refs.contains(&101));
}

#[test]
fn direct_face_use_lattice_rejects_a_nonreciprocal_use() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second use node")
        .refs[2] = 1;

    assert!(
        disc1b_disc1d_disc19_disc15_disc13_disc11_disc0f_direct_face_root_body(&index_records(
            &records,
        ))
        .is_empty()
    );
}

#[test]
fn direct_face_use_lattice_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("disc11 chain record")
        .refs[2] = 1;

    assert!(
        disc1b_disc1d_disc19_disc15_disc13_disc11_disc0f_direct_face_root_body(&index_records(
            &records,
        ))
        .is_empty()
    );
}
