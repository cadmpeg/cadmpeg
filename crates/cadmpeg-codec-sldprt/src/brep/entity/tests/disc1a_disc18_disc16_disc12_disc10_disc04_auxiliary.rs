use super::super::disc1a_disc18_disc16_disc12_disc10_disc04_auxiliary_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x12, [3, 12, 14, 1, 1, 1]),
        record(14, 0x10, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
        record(20, 0x0e, [1, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [1, 31, 1, 1, 1, 1]),
        record(30, 0x14, [1, 40, 20, 1, 1, 1]),
        record(31, 0x14, [1, 41, 21, 1, 1, 1]),
        flo4(40, 0x1c, [1; 6]),
        flo4(41, 0x1c, [1; 6]),
        flo4(42, 0x1c, [1; 6]),
    ]
}

#[test]
fn auxiliary_chain_accepts_extra_use_nodes() {
    let records = lattice();
    let bodies = disc1a_disc18_disc16_disc12_disc10_disc04_auxiliary_face_root_body(
        &index_records(&records),
        &records,
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc1a-disc18-disc16-disc12-disc10-disc04 body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn auxiliary_chain_rejects_a_missing_face_use() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 30)
        .collect::<Vec<_>>();

    assert!(
        disc1a_disc18_disc16_disc12_disc10_disc04_auxiliary_face_root_body(
            &index_records(&records),
            &records,
        )
        .is_empty()
    );
}

#[test]
fn auxiliary_chain_rejects_a_missing_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 41)
        .collect::<Vec<_>>();

    assert!(
        disc1a_disc18_disc16_disc12_disc10_disc04_auxiliary_face_root_body(
            &index_records(&records),
            &records,
        )
        .is_empty()
    );
}

#[test]
fn auxiliary_chain_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 13)
        .expect("disc12 chain record")
        .refs[2] = 1;

    assert!(
        disc1a_disc18_disc16_disc12_disc10_disc04_auxiliary_face_root_body(
            &index_records(&records),
            &records,
        )
        .is_empty()
    );
}
