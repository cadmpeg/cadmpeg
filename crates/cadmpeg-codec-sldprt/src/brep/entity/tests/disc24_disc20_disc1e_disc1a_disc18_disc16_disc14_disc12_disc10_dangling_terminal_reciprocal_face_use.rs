use super::super::disc24_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc10_dangling_terminal_reciprocal_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x24, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        record(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x18, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x16, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x14, [3, 15, 17, 1, 1, 1]),
        flo2(17, 0x12, [3, 16, 18, 1, 1, 1]),
        flo2(18, 0x10, [3, 17, 39, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 21, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 31, 1, 1, 1]),
    ]
}

#[test]
fn dangling_terminal_lattice_owns_reciprocal_faces() {
    let bodies =
        disc24_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc10_dangling_terminal_reciprocal_face_root_body(
            &index_records(&lattice()),
        );
    let [body] = bodies.as_slice() else {
        panic!("one dangling-terminal reciprocal face-use body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 15);
    for attr in [20, 21, 30, 31, 40, 41] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
        assert!(
            body.regions[0].shells[0].refs.contains(&attr),
            "missing shell reference {attr}"
        );
    }
    assert!(!body.refs.contains(&39));
}

#[test]
fn dangling_terminal_lattice_rejects_a_present_terminal_target() {
    let mut records = lattice();
    records.push(flo2(39, 0x10, [3, 18, 1, 1, 1, 1]));

    assert!(
        disc24_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc10_dangling_terminal_reciprocal_face_root_body(
            &index_records(&records),
        )
        .is_empty()
    );
}

#[test]
fn dangling_terminal_lattice_rejects_a_broken_companion_backlink() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[2] = 1;

    assert!(
        disc24_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc10_dangling_terminal_reciprocal_face_root_body(
            &index_records(&records),
        )
        .is_empty()
    );
}

#[test]
fn dangling_terminal_lattice_rejects_a_broken_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 17)
        .expect("disc12 chain record")
        .refs[2] = 99;

    assert!(
        disc24_disc20_disc1e_disc1a_disc18_disc16_disc14_disc12_disc10_dangling_terminal_reciprocal_face_root_body(
            &index_records(&records),
        )
        .is_empty()
    );
}
