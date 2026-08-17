use super::super::disc20_disc1a_disc18_disc16_disc14_disc12_disc04_keyed_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
        record(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(30, 0x10, [100, 40, 20, 1, 1, 1]),
        record(31, 0x10, [101, 41, 21, 1, 1, 1]),
        record(40, 0x1e, [100, 50, 30, 1, 1, 1]),
        record(41, 0x1e, [101, 51, 31, 1, 1, 1]),
        flo4(50, 0x22, [100, 1, 40, 1, 1, 1]),
        flo4(51, 0x22, [101, 1, 41, 1, 1, 1]),
        record(42, 0x1e, [102, 52, 1, 1, 1, 1]),
        flo4(52, 0x22, [102, 1, 42, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_the_terminal_disc04_site() {
    let records = lattice();
    let bodies = disc20_disc1a_disc18_disc16_disc14_disc12_disc04_keyed_face_root_body(
        &index_records(&records),
    );
    let [body] = bodies.as_slice() else {
        panic!("one keyed disc20-disc1a-disc18-disc16-disc14-disc12-disc04 body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 12));
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
    assert!(body.refs.contains(&50) && body.refs.contains(&51));
}

#[test]
fn keyed_lattice_ignores_unselected_face_use_and_use_nodes() {
    let records = lattice();
    let bodies = disc20_disc1a_disc18_disc16_disc14_disc12_disc04_keyed_face_root_body(
        &index_records(&records),
    );
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].refs.contains(&42) && bodies[0].refs.contains(&52));
}

#[test]
fn keyed_lattice_rejects_a_missing_face_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 51)
        .collect::<Vec<_>>();
    assert!(
        disc20_disc1a_disc18_disc16_disc14_disc12_disc04_keyed_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_broken_chain_predecessor() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 14)
        .expect("chain record")
        .refs[1] = 99;
    assert!(
        disc20_disc1a_disc18_disc16_disc14_disc12_disc04_keyed_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}
