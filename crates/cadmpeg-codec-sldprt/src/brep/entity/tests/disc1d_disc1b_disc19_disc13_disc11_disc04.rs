use super::super::{
    disc19_disc1b_disc1d_disc11_disc13_disc04_face_root_body,
    disc1d_disc1b_disc19_disc13_disc11_disc04_face_root_body,
};
use super::{flo2, flo4, index_records, record};

fn lattice(chain: Chain) -> Vec<super::super::EntityRecord> {
    let chain_records = match chain {
        Chain::Disc1d => [
            flo2(10, 0x1d, [7, 1, 11, 1, 1, 1]),
            flo2(11, 0x1b, [7, 10, 12, 1, 1, 1]),
            flo2(12, 0x19, [7, 11, 13, 1, 1, 1]),
            record(13, 0x13, [7, 12, 14, 1, 1, 1]),
            flo2(14, 0x11, [7, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [7, 14, 1, 1, 1, 1]),
        ],
        Chain::Disc19 => [
            flo2(10, 0x19, [7, 1, 11, 1, 1, 1]),
            flo2(11, 0x1b, [7, 10, 12, 1, 1, 1]),
            flo2(12, 0x1d, [7, 11, 13, 1, 1, 1]),
            flo2(13, 0x11, [7, 12, 14, 1, 1, 1]),
            record(14, 0x13, [7, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [7, 14, 1, 1, 1, 1]),
        ],
    };

    chain_records
        .into_iter()
        .chain([
            record(20, 0x0f, [100, 30, 1, 1, 1, 1]),
            record(21, 0x0f, [101, 31, 1, 1, 1, 1]),
            record(30, 0x15, [100, 40, 20, 1, 1, 1]),
            record(31, 0x15, [101, 41, 21, 1, 1, 1]),
            record(32, 0x15, [999, 1, 1, 1, 1, 1]),
            flo4(40, 0x17, [100, 1, 30, 1, 1, 1]),
            flo4(41, 0x17, [101, 1, 31, 1, 1, 1]),
        ])
        .collect()
}

#[derive(Clone, Copy)]
enum Chain {
    Disc1d,
    Disc19,
}

#[test]
fn disc1d_chain_accepts_reciprocal_faces_and_extra_companions() {
    let bodies = disc1d_disc1b_disc19_disc13_disc11_disc04_face_root_body(&index_records(
        &lattice(Chain::Disc1d),
    ));
    let [body] = bodies.as_slice() else {
        panic!("one disc1d-disc1b-disc19-disc13-disc11-disc04 body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 13));
    assert!(body.refs.contains(&32));
    assert!(
        disc19_disc1b_disc1d_disc11_disc13_disc04_face_root_body(&index_records(&lattice(
            Chain::Disc1d
        )))
        .is_empty()
    );
}

#[test]
fn disc19_chain_accepts_the_permuted_keyed_face_lattice() {
    let bodies = disc19_disc1b_disc1d_disc11_disc13_disc04_face_root_body(&index_records(
        &lattice(Chain::Disc19),
    ));
    let [body] = bodies.as_slice() else {
        panic!("one disc19-disc1b-disc1d-disc11-disc13-disc04 body");
    };
    assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 14));
    assert!(body.refs.contains(&32));
    assert!(
        disc1d_disc1b_disc19_disc13_disc11_disc04_face_root_body(&index_records(&lattice(
            Chain::Disc19
        )))
        .is_empty()
    );
}

#[test]
fn keyed_face_lattice_rejects_an_extra_use_node() {
    let mut records = lattice(Chain::Disc1d);
    records.push(flo4(42, 0x17, [999, 1, 32, 1, 1, 1]));

    assert!(
        disc1d_disc1b_disc19_disc13_disc11_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_face_lattice_rejects_a_broken_companion_backlink() {
    let mut records = lattice(Chain::Disc19);
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[2] = 1;

    assert!(
        disc19_disc1b_disc1d_disc11_disc13_disc04_face_root_body(&index_records(&records))
            .is_empty()
    );
}
