use super::super::record_lattice::{
    disc1e_disc1c_disc18_disc10_reciprocal_root_body,
    disc1e_disc1c_disc1a_disc18_disc16_shared_use_root_body,
    disc20_disc1e_disc1c_disc18_disc16_disc12_root_body,
};
use super::{flo2, flo4, record};

fn shared_use_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1c, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [7, 12, 14, 1, 1, 1]),
        flo2(14, 0x16, [7, 13, 15, 1, 1, 1]),
        flo2(15, 0x14, [7, 14, 1, 1, 1, 1]),
        record(20, 0x0e, [101, 40, 1, 1, 1, 1]),
        record(20, 0x0e, [101, 40, 1, 1, 1, 1]),
        record(21, 0x0e, [102, 41, 1, 1, 1, 1]),
        record(30, 0x10, [101, 40, 1, 1, 1, 1]),
        record(31, 0x10, [102, 41, 1, 1, 1, 1]),
        flo4(40, 0x20, [101, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [102, 1, 31, 1, 1, 1]),
        flo4(42, 0x20, [999, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn shared_use_lattice_composes_duplicate_views_and_unselected_nodes() {
    let bodies = disc1e_disc1c_disc1a_disc18_disc16_shared_use_root_body(&shared_use_lattice());
    let [body] = bodies.as_slice() else {
        panic!("one shared-use lattice body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 14));
    for attr in [20, 21, 30, 31, 40, 41, 42, 101, 102] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
        assert!(
            body.regions[0].shells[0].refs.contains(&attr),
            "missing shell reference {attr}"
        );
    }
    assert!(!body.refs.contains(&999));
}

#[test]
fn shared_use_lattice_rejects_a_nonshared_use_link() {
    let mut records = shared_use_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[1] = 40;

    assert!(disc1e_disc1c_disc1a_disc18_disc16_shared_use_root_body(&records).is_empty());
}

fn reciprocal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(100, 0x1e, [9, 1, 101, 1, 1, 1]),
        flo2(101, 0x1c, [9, 100, 102, 1, 1, 1]),
        flo2(102, 0x18, [9, 101, 103, 1, 1, 1]),
        flo2(103, 0x10, [9, 102, 1, 1, 1, 1]),
        record(200, 0x12, [201, 210, 1, 1, 1, 1]),
        record(201, 0x12, [202, 211, 1, 1, 1, 1]),
        record(210, 0x1a, [201, 220, 200, 1, 1, 1]),
        record(211, 0x1a, [202, 221, 201, 1, 1, 1]),
        flo4(220, 0x20, [201, 1, 210, 1, 1, 1]),
        flo4(221, 0x20, [202, 1, 211, 1, 1, 1]),
        record(230, 0x1a, [999, 1, 1, 1, 1, 1]),
        flo4(231, 0x20, [999, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn reciprocal_lattice_owns_multiple_keyed_faces() {
    let bodies = disc1e_disc1c_disc18_disc10_reciprocal_root_body(&reciprocal_lattice());
    let [body] = bodies.as_slice() else {
        panic!("one reciprocal lattice body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (100, 102));
    for attr in [200, 201, 210, 211, 220, 221, 230, 231, 201, 202] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
    }
}

#[test]
fn reciprocal_lattice_rejects_a_broken_use_backlink() {
    let mut records = reciprocal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 221)
        .expect("second use node")
        .refs[2] = 200;

    assert!(disc1e_disc1c_disc18_disc10_reciprocal_root_body(&records).is_empty());
}

fn bridged_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(500, 0x20, [9, 1, 501, 1, 1, 1]),
        flo2(501, 0x1e, [9, 500, 502, 1, 1, 1]),
        flo2(502, 0x1c, [9, 501, 503, 1, 1, 1]),
        flo2(503, 0x18, [9, 502, 504, 1, 1, 1]),
        record(504, 0x16, [9, 503, 505, 1, 1, 1]),
        flo2(505, 0x12, [9, 504, 6, 1, 1, 1]),
        record(600, 0x10, [150, 601, 1, 1, 1, 1]),
        flo2(601, 0x14, [150, 603, 600, 1, 1, 1]),
        record(603, 0x1a, [150, 604, 601, 1, 1, 1]),
        flo4(604, 0x22, [150, 1, 603, 1, 1, 1]),
        record(610, 0x10, [151, 611, 1, 1, 1, 1]),
        record(611, 0x1a, [151, 613, 610, 1, 1, 1]),
        flo4(613, 0x22, [151, 1, 611, 1, 1, 1]),
        record(620, 0x1a, [999, 621, 1, 1, 1, 1]),
        flo4(621, 0x22, [999, 1, 620, 1, 1, 1]),
    ]
}

#[test]
fn non_entity_terminal_lattice_accepts_a_unique_face_bridge() {
    let bodies = disc20_disc1e_disc1c_disc18_disc16_disc12_root_body(&bridged_lattice());
    let [body] = bodies.as_slice() else {
        panic!("one bridged non-entity-terminal body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (500, 504));
    for attr in [600, 601, 603, 604, 610, 611, 613, 620, 621, 150, 151] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
    }
    assert!(!body.refs.contains(&6));
}

#[test]
fn non_entity_terminal_lattice_rejects_a_same_key_terminal_record() {
    let mut records = bridged_lattice();
    records.push(flo2(6, 0x12, [9, 505, 1, 1, 1, 1]));

    assert!(disc20_disc1e_disc1c_disc18_disc16_disc12_root_body(&records).is_empty());
}

#[test]
fn non_entity_terminal_lattice_rejects_a_broken_bridge() {
    let mut records = bridged_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 603)
        .expect("bridged companion")
        .refs[2] = 600;

    assert!(disc20_disc1e_disc1c_disc18_disc16_disc12_root_body(&records).is_empty());
}
