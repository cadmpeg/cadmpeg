use super::super::disc2a_disc26_disc24_disc1e_disc1c_disc18_keyed_population_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x2a, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x26, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x24, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1e, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x1c, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x18, [3, 14, 1, 1, 1, 1]),
        record(20, 0x12, [100, 1, 1, 1, 1, 1]),
        record(21, 0x12, [101, 1, 1, 1, 1, 1]),
        record(30, 0x22, [100, 1, 1, 1, 1, 1]),
        record(31, 0x22, [101, 1, 1, 1, 1, 1]),
        record(32, 0x22, [100, 1, 1, 1, 1, 1]),
        flo4(40, 0x28, [100, 1, 1, 1, 1, 1]),
        flo4(41, 0x28, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x28, [999, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_population_accepts_repeated_keys_and_extra_records() {
    let bodies = disc2a_disc26_disc24_disc1e_disc1c_disc18_keyed_population_face_root_body(
        &index_records(&lattice()),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc2a-disc26-disc24-disc1e-disc1c-disc18 body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    for attr in [20, 21, 30, 31, 32, 40, 41, 42] {
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
        disc2a_disc26_disc24_disc1e_disc1c_disc18_keyed_population_face_root_body(&index_records(
            &records
        ))
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
        disc2a_disc26_disc24_disc1e_disc1c_disc18_keyed_population_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}

#[test]
fn keyed_population_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 14)
        .expect("disc1c chain record")
        .refs[2] = 1;

    assert!(
        disc2a_disc26_disc24_disc1e_disc1c_disc18_keyed_population_face_root_body(&index_records(
            &records
        ))
        .is_empty()
    );
}
