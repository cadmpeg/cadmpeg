use super::super::disc22_disc28_disc20_disc04_disc1a_disc1e_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x28, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [3, 11, 13, 1, 1, 1]),
        record(13, 0x04, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x1a, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x1e, [3, 14, 1, 1, 1, 1]),
        record(20, 0x10, [100, 30, 1, 1, 1, 1]),
        record(21, 0x10, [101, 31, 1, 1, 1, 1]),
        record(22, 0x10, [100, 32, 1, 1, 1, 1]),
        record(23, 0x10, [102, 33, 1, 1, 1, 1]),
        record(30, 0x26, [100, 40, 1, 1, 1, 1]),
        record(31, 0x26, [101, 41, 1, 1, 1, 1]),
        record(32, 0x26, [100, 42, 1, 1, 1, 1]),
        record(33, 0x26, [102, 43, 1, 1, 1, 1]),
        flo4(40, 0x2a, [100, 1, 1, 1, 1, 1]),
        flo4(41, 0x2a, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x2a, [100, 1, 1, 1, 1, 1]),
        flo4(43, 0x2a, [102, 1, 1, 1, 1, 1]),
        flo4(44, 0x2a, [999, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_population_accepts_repeated_keys_and_extra_records() {
    let bodies =
        disc22_disc28_disc20_disc04_disc1a_disc1e_face_root_body(&index_records(&lattice()));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc28-disc20-disc04-disc1a-disc1e body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    for attr in [20, 23, 30, 33, 40, 44] {
        assert!(body.refs.contains(&attr), "missing body reference {attr}");
        assert!(
            body.regions[0].shells[0].refs.contains(&attr),
            "missing shell reference {attr}"
        );
    }
}

#[test]
fn keyed_population_rejects_a_missing_companion_key() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 31)
        .collect::<Vec<_>>();

    assert!(
        disc22_disc28_disc20_disc04_disc1a_disc1e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_population_rejects_a_missing_use_key() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 41)
        .collect::<Vec<_>>();

    assert!(
        disc22_disc28_disc20_disc04_disc1a_disc1e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_population_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 14)
        .expect("disc1a chain record")
        .refs[2] = 1;

    assert!(
        disc22_disc28_disc20_disc04_disc1a_disc1e_face_root_body(&index_records(&records))
            .is_empty()
    );
}
