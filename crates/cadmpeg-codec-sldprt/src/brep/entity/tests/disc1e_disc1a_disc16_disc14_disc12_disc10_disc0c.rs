use super::super::disc1e_disc1a_disc16_disc14_disc12_disc10_disc0c_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
        record(12, 0x16, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x0c, [3, 15, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 20, 1, 1, 1]),
        record(31, 0x18, [101, 41, 21, 1, 1, 1]),
        flo4(40, 0x1c, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1c, [101, 1, 31, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_the_terminal_disc0c_site() {
    let bodies =
        disc1e_disc1a_disc16_disc14_disc12_disc10_disc0c_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1a-disc16-disc14-disc12-disc10-disc0c body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn keyed_lattice_requires_equal_keyed_face_and_use_population() {
    let mut records = lattice();
    records.push(flo4(42, 0x1c, [102, 1, 1, 1, 1, 1]));
    assert!(
        disc1e_disc1a_disc16_disc14_disc12_disc10_disc0c_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_missing_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 41)
        .collect::<Vec<_>>();
    assert!(
        disc1e_disc1a_disc16_disc14_disc12_disc10_disc0c_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_broken_reciprocal_use_link() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 40)
        .expect("first use node")
        .refs[2] = 99;
    assert!(
        disc1e_disc1a_disc16_disc14_disc12_disc10_disc0c_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_broken_chain_predecessor() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 14)
        .expect("disc12 chain record")
        .refs[1] = 99;
    assert!(
        disc1e_disc1a_disc16_disc14_disc12_disc10_disc0c_face_root_body(&index_records(&records))
            .is_empty()
    );
}
