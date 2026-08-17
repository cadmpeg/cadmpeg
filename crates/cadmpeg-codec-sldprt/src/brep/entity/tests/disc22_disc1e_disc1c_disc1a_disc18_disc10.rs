use super::super::disc22_disc1e_disc1c_disc1a_disc18_disc10_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x18, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 1, 1, 1, 1]),
        record(20, 0x12, [100, 30, 1, 1, 1, 1]),
        record(21, 0x12, [101, 31, 1, 1, 1, 1]),
        record(30, 0x20, [100, 40, 1, 1, 1, 1]),
        record(31, 0x20, [101, 41, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 1, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn forward_keyed_lattice_owns_the_disc1c_shell() {
    let bodies =
        disc22_disc1e_disc1c_disc1a_disc18_disc10_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc1e-disc1c-disc1a-disc18-disc10 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn forward_keyed_lattice_ignores_unselected_companions_and_use_nodes() {
    let mut records = lattice();
    records.push(record(32, 0x20, [102, 42, 1, 1, 1, 1]));
    records.push(flo4(42, 0x24, [102, 1, 1, 1, 1, 1]));
    assert_eq!(
        disc22_disc1e_disc1c_disc1a_disc18_disc10_face_root_body(&index_records(&records)).len(),
        1
    );
}

#[test]
fn forward_keyed_lattice_rejects_a_missing_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 41)
        .collect::<Vec<_>>();
    assert!(
        disc22_disc1e_disc1c_disc1a_disc18_disc10_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn forward_keyed_lattice_rejects_a_broken_chain_predecessor() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 13)
        .expect("disc1a chain record")
        .refs[1] = 99;
    assert!(
        disc22_disc1e_disc1c_disc1a_disc18_disc10_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn forward_keyed_lattice_rejects_a_broken_forward_companion_link() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first face record")
        .refs[1] = 1;
    assert!(
        disc22_disc1e_disc1c_disc1a_disc18_disc10_face_root_body(&index_records(&records))
            .is_empty()
    );
}
