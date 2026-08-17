use super::super::{
    disc1e_disc10_disc1c_disc1a_disc0e_face_root_body,
    disc1e_disc16_disc1c_disc1a_disc14_face_root_body,
    disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body,
};
use super::{flo2, flo4, index_records, record};

fn lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
        record(16, 0x0e, [3, 15, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(22, 0x04, [102, 50, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 20, 1, 1, 1]),
        record(31, 0x18, [101, 41, 21, 1, 1, 1]),
        record(51, 0x18, [102, 52, 50, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 31, 1, 1, 1]),
        flo4(52, 0x20, [102, 1, 51, 1, 1, 1]),
        flo2(50, 0x10, [102, 51, 22, 1, 1, 1]),
    ]
}

#[test]
fn keyed_lattice_owns_directly_linked_faces() {
    let records = lattice();
    let bodies = disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc1c-disc1a-disc16-disc14-disc12 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&40) && body.refs.contains(&41) && body.refs.contains(&52));
}

#[test]
fn keyed_lattice_rejects_broken_use_back_reference() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 41)
        .expect("second use node")
        .refs[2] = 1;

    assert!(
        disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body(&index_records(&records))
            .is_empty()
    );
}

#[test]
fn keyed_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[0] = 102;

    assert!(
        disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body(&index_records(&records))
            .is_empty()
    );
}

fn reordered_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x16, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        record(14, 0x14, [3, 13, 1, 1, 1, 1]),
        record(20, 0x0c, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0c, [101, 31, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 20, 1, 1, 1]),
        record(31, 0x18, [101, 41, 21, 1, 1, 1]),
        record(32, 0x18, [102, 1, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 1, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 31, 1, 1, 1]),
    ]
}

#[test]
fn reordered_lattice_owns_forward_keyed_use_links_and_unselected_companions() {
    let records = reordered_lattice();
    let bodies = disc1e_disc16_disc1c_disc1a_disc14_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc16-disc1c-disc1a-disc14 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 11);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn reordered_lattice_accepts_stale_reverse_face_and_use_links_by_key() {
    let mut records = reordered_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[2] = 1;
    records
        .iter_mut()
        .find(|record| record.attr == 40)
        .expect("first use node")
        .refs[2] = 1;

    let bodies = disc1e_disc16_disc1c_disc1a_disc14_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn reordered_lattice_rejects_an_extra_use_node() {
    let mut records = reordered_lattice();
    records.push(flo4(52, 0x20, [102, 1, 1, 1, 1, 1]));

    assert!(disc1e_disc16_disc1c_disc1a_disc14_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn reordered_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = reordered_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc1e_disc16_disc1c_disc1a_disc14_face_root_body(&index_records(&records)).is_empty());
}

fn disc10_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x10, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x1a, [3, 12, 14, 1, 1, 1]),
        record(14, 0x0e, [3, 13, 1, 1, 1, 1]),
        record(20, 0x14, [100, 30, 1, 1, 1, 1]),
        record(21, 0x14, [101, 31, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 20, 1, 1, 1]),
        record(31, 0x18, [101, 41, 21, 1, 1, 1]),
        record(32, 0x18, [102, 1, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc10_lattice_owns_forward_keyed_use_links_and_unselected_companions() {
    let records = disc10_lattice();
    let bodies = disc1e_disc10_disc1c_disc1a_disc0e_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc10-disc1c-disc1a-disc0e body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 11);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));
}

#[test]
fn disc10_lattice_accepts_stale_reverse_face_and_use_links_by_key() {
    let mut records = disc10_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 30)
        .expect("first companion")
        .refs[2] = 1;
    records
        .iter_mut()
        .find(|record| record.attr == 40)
        .expect("first use node")
        .refs[2] = 1;

    let bodies = disc1e_disc10_disc1c_disc1a_disc0e_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc10_lattice_rejects_an_extra_use_node() {
    let mut records = disc10_lattice();
    records.push(flo4(52, 0x20, [102, 1, 1, 1, 1, 1]));

    assert!(disc1e_disc10_disc1c_disc1a_disc0e_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc10_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc10_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc1e_disc10_disc1c_disc1a_disc0e_face_root_body(&index_records(&records)).is_empty());
}
