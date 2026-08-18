use super::super::disc20_disc1e_disc1c_disc18_disc14_disc10_disc0e_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
        record(16, 0x0e, [3, 15, 1, 1, 1, 1]),
        record(20, 0x16, [100, 40, 1, 1, 1, 1]),
        record(21, 0x16, [101, 1, 1, 1, 1, 1]),
        record(40, 0x1a, [100, 50, 20, 1, 1, 1]),
        record(41, 0x1a, [101, 60, 1, 1, 1, 1]),
        flo4(50, 0x22, [100, 1, 40, 1, 1, 1]),
        flo4(60, 0x22, [101, 1, 41, 1, 1, 1]),
        flo4(52, 0x22, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_chain_accepts_reciprocal_and_keyed_links_with_extra_use_nodes() {
    let records = lattice();
    let bodies =
        disc20_disc1e_disc1c_disc18_disc14_disc10_disc0e_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc1e-disc1c-disc18-disc14-disc10-disc0e body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
    assert!(body.refs.contains(&50) && body.refs.contains(&52));
}

#[test]
fn keyed_chain_rejects_a_missing_keyed_companion() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 41)
        .collect::<Vec<_>>();

    assert!(
        disc20_disc1e_disc1c_disc18_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_chain_rejects_a_missing_keyed_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 60)
        .collect::<Vec<_>>();

    assert!(
        disc20_disc1e_disc1c_disc18_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_chain_rejects_an_unselected_companion() {
    let mut records = lattice();
    records.push(record(42, 0x1a, [102, 70, 1, 1, 1, 1]));

    assert!(
        disc20_disc1e_disc1c_disc18_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_chain_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 14)
        .expect("disc14 shell record")
        .refs[2] = 1;

    assert!(
        disc20_disc1e_disc1c_disc18_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}
