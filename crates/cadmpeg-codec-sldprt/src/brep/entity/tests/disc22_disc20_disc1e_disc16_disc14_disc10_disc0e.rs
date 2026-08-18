use super::super::disc22_disc20_disc1e_disc16_disc14_disc10_disc0e_face_root_body;
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
        record(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x0e, [3, 15, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 99, 1, 1, 1, 1]),
        record(22, 0x04, [102, 98, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1c, [103, 42, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 31, 1, 1, 1]),
        flo4(42, 0x24, [103, 1, 32, 1, 1, 1]),
    ]
}

#[test]
fn keyed_layout_selects_faces_and_keeps_unselected_population() {
    let records = lattice();
    let bodies =
        disc22_disc20_disc1e_disc16_disc14_disc10_disc0e_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc20-disc1e-disc16-disc14-disc10-disc0e body");
    };

    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&32));
    assert!(body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn keyed_layout_rejects_a_site_without_a_selected_face() {
    let records = lattice()
        .into_iter()
        .filter(|record| record.disc != 0x0004 || record.attr == 22)
        .collect::<Vec<_>>();

    assert!(
        disc22_disc20_disc1e_disc16_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_layout_rejects_a_site_without_a_use_node() {
    let records = lattice()
        .into_iter()
        .filter(|record| !matches!(record.attr, 40..=42))
        .collect::<Vec<_>>();

    assert!(
        disc22_disc20_disc1e_disc16_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_layout_rejects_an_incomplete_chain() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 16)
        .expect("terminal chain record")
        .refs[2] = 17;

    assert!(
        disc22_disc20_disc1e_disc16_disc14_disc10_disc0e_face_root_body(&index_records(&records))
            .is_empty()
    );
}
