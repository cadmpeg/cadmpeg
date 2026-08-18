use super::super::disc1c_disc18_disc16_disc14_disc12_disc0e_disc04_chain_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
        record(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x10, [100, 40, 1, 1, 1, 1]),
        record(21, 0x10, [100, 41, 1, 1, 1, 1]),
        record(22, 0x10, [101, 42, 1, 1, 1, 1]),
        record(40, 0x1a, [100, 1, 1, 1, 1, 1]),
        record(41, 0x1a, [100, 1, 1, 1, 1, 1]),
        record(42, 0x1a, [101, 1, 1, 1, 1, 1]),
        record(43, 0x1a, [102, 1, 1, 1, 1, 1]),
        flo4(50, 0x1e, [100, 1, 1, 1, 1, 1]),
        flo4(51, 0x1e, [101, 1, 1, 1, 1, 1]),
        flo4(52, 0x1e, [102, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn keyed_chain_accepts_repeated_keys_and_extra_population_records() {
    let records = lattice();
    let bodies =
        disc1c_disc18_disc16_disc14_disc12_disc0e_disc04_chain_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc18-disc16-disc14-disc12-disc0e-disc04 body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&22));
    assert!(body.refs.contains(&40) && body.refs.contains(&43));
    assert!(body.refs.contains(&50) && body.refs.contains(&52));
}

#[test]
fn keyed_chain_rejects_a_missing_companion_key() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 42)
        .collect::<Vec<_>>();

    assert!(
        disc1c_disc18_disc16_disc14_disc12_disc0e_disc04_chain_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_chain_rejects_a_missing_use_key() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.attr != 51)
        .collect::<Vec<_>>();

    assert!(
        disc1c_disc18_disc16_disc14_disc12_disc0e_disc04_chain_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_chain_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 15)
        .expect("terminal predecessor")
        .refs[2] = 1;

    assert!(
        disc1c_disc18_disc16_disc14_disc12_disc0e_disc04_chain_body(&index_records(&records))
            .is_empty()
    );
}
