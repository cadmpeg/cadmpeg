use super::super::disc22_disc20_disc1e_disc1a_disc18_disc16_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x18, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x16, [3, 14, 1, 1, 1, 1]),
        record(20, 0x10, [3, 30, 1, 1, 1, 1]),
        record(21, 0x10, [3, 31, 1, 1, 1, 1]),
        record(30, 0x1c, [3, 40, 1, 1, 1, 1]),
        record(31, 0x1c, [3, 41, 1, 1, 1, 1]),
        flo4(40, 0x24, [3, 1, 1, 1, 1, 1]),
        flo4(41, 0x24, [3, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn forward_keyed_lattice_owns_the_disc1e_shell() {
    let mut records = lattice();
    records.push(record(32, 0x1c, [3, 42, 1, 1, 1, 1]));
    records.push(flo4(42, 0x24, [3, 1, 1, 1, 1, 1]));

    let bodies = disc22_disc20_disc1e_disc1a_disc18_disc16_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc20-disc1e-disc1a-disc18-disc16 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn forward_keyed_lattice_rejects_ambiguous_companion_fallback() {
    let mut records = lattice();
    records.push(record(32, 0x1c, [3, 42, 1, 1, 1, 1]));
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[1] = 1;

    assert!(
        disc22_disc20_disc1e_disc1a_disc18_disc16_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn forward_keyed_lattice_rejects_a_use_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 40)
        .expect("first use node")
        .refs[0] = 7;

    assert!(
        disc22_disc20_disc1e_disc1a_disc18_disc16_face_root_body(&index_records(&records))
            .is_empty()
    );
}
