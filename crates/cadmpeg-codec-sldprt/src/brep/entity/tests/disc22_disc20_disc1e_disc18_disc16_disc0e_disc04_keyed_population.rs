use super::super::disc22_disc20_disc1e_disc18_disc16_disc0e_disc04_keyed_population_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x16, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
        record(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x10, [100, 30, 1, 1, 1, 1]),
        record(21, 0x10, [101, 31, 1, 1, 1, 1]),
        record(22, 0x10, [100, 32, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 21, 1, 1, 1]),
        record(32, 0x1c, [100, 42, 22, 1, 1, 1]),
        record(33, 0x1c, [102, 43, 1, 1, 1, 1]),
        record(34, 0x1a, [100, 1, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x24, [100, 1, 32, 1, 1, 1]),
        flo4(43, 0x24, [102, 1, 33, 1, 1, 1]),
    ]
}

#[test]
fn keyed_population_accepts_repeated_keys_and_extra_records() {
    let records = lattice();
    let bodies = disc22_disc20_disc1e_disc18_disc16_disc0e_disc04_keyed_population_face_root_body(
        &index_records(&records),
    );
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc20-disc1e-disc18-disc16-disc0e-disc04 body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&33));
    assert!(body.refs.contains(&40) && body.refs.contains(&43));
}

#[test]
fn keyed_population_rejects_a_missing_companion_key() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 31 && record.attr != 32)
        .collect::<Vec<_>>();

    assert!(
        disc22_disc20_disc1e_disc18_disc16_disc0e_disc04_keyed_population_face_root_body(
            &index_records(&records)
        )
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
        disc22_disc20_disc1e_disc18_disc16_disc0e_disc04_keyed_population_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}

#[test]
fn keyed_population_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("terminal predecessor")
        .refs[2] = 1;

    assert!(
        disc22_disc20_disc1e_disc18_disc16_disc0e_disc04_keyed_population_face_root_body(
            &index_records(&records)
        )
        .is_empty()
    );
}
