use super::super::{
    disc16_disc12_disc1a_disc14_disc10_face_root_body,
    disc1a_disc0e_disc1e_disc18_disc04_face_root_body,
    disc1a_disc16_disc1e_disc18_disc14_face_root_body,
    disc1c_disc14_disc1a_disc18_disc10_face_root_body,
    disc1c_disc16_disc24_disc1a_disc18_face_root_body,
    disc1e_disc10_disc1c_disc1a_disc0e_face_root_body,
    disc1e_disc16_disc1c_disc1a_disc14_face_root_body,
    disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body,
    disc20_disc10_disc1e_disc1c_disc18_face_root_body,
    disc20_disc12_disc1e_disc1c_disc04_face_root_body,
    disc20_disc12_disc1e_disc1c_disc10_face_root_body,
    disc20_disc12_disc1e_disc1c_disc14_face_root_body,
    disc20_disc16_disc26_disc1e_disc14_face_root_body,
    disc22_disc16_disc1e_disc1c_disc18_face_root_body,
    disc22_disc16_disc20_disc1e_disc04_face_root_body,
    disc22_disc18_disc20_disc1e_disc04_face_root_body,
    disc22_disc1a_disc20_disc1e_disc04_face_root_body,
    disc26_disc1e_disc24_disc22_disc04_face_root_body,
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

fn disc20_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x16, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x26, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1e, [7, 12, 14, 1, 1, 1]),
        record(14, 0x14, [7, 13, 1, 1, 1, 1]),
        record(20, 0x06, [100, 30, 1, 1, 1, 1]),
        record(21, 0x06, [101, 31, 1, 1, 1, 1]),
        record(22, 0x06, [102, 50, 1, 1, 1, 1]),
        record(30, 0x24, [100, 40, 20, 1, 1, 1]),
        record(31, 0x24, [101, 41, 1, 1, 1, 1]),
        record(32, 0x24, [102, 42, 22, 1, 1, 1]),
        record(33, 0x24, [103, 43, 1, 1, 1, 1]),
        flo4(40, 0x28, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x28, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x28, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x28, [103, 1, 33, 1, 1, 1]),
        flo4(44, 0x28, [104, 1, 1, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc20_lattice_owns_forward_and_keyed_faces_with_unselected_records() {
    let records = disc20_lattice();
    let bodies = disc20_disc16_disc26_disc1e_disc14_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc16-disc26-disc1e-disc14 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 11);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&44));
}

#[test]
fn disc20_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc20_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc20_disc16_disc26_disc1e_disc14_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc20_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc20_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc20_disc16_disc26_disc1e_disc14_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc20_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc20_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;

    assert!(disc20_disc16_disc26_disc1e_disc14_face_root_body(&index_records(&records)).is_empty());
}

fn disc18_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x10, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [7, 12, 14, 1, 1, 1]),
        record(14, 0x18, [7, 13, 1, 1, 1, 1]),
        record(20, 0x14, [100, 30, 1, 1, 1, 1]),
        record(21, 0x14, [101, 31, 1, 1, 1, 1]),
        record(22, 0x14, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1a, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1a, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc18_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc18_terminal_lattice();
    let bodies = disc20_disc10_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc10-disc1e-disc1c-disc18 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc18_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc18_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc20_disc10_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc18_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc18_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc20_disc10_disc1e_disc1c_disc18_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc18_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc18_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc20_disc10_disc1e_disc1c_disc18_face_root_body(&index_records(&records)).is_empty());
}

fn disc26_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x26, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1e, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x24, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x22, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x10, [100, 30, 1, 1, 1, 1]),
        record(21, 0x10, [101, 31, 1, 1, 1, 1]),
        record(22, 0x10, [102, 50, 1, 1, 1, 1]),
        record(30, 0x20, [100, 40, 20, 1, 1, 1]),
        record(31, 0x20, [101, 41, 1, 1, 1, 1]),
        record(32, 0x20, [102, 42, 22, 1, 1, 1]),
        record(33, 0x20, [103, 43, 1, 1, 1, 1]),
        flo4(40, 0x28, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x28, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x28, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x28, [103, 1, 33, 1, 1, 1]),
        flo4(44, 0x28, [104, 1, 1, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc26_lattice_owns_forward_and_keyed_faces_with_unselected_records() {
    let records = disc26_lattice();
    let bodies = disc26_disc1e_disc24_disc22_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc26-disc1e-disc24-disc22-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 11);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&44));
}

#[test]
fn disc26_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc26_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc26_disc1e_disc24_disc22_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc26_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc26_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc26_disc1e_disc24_disc22_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc26_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc26_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;

    assert!(disc26_disc1e_disc24_disc22_disc04_face_root_body(&index_records(&records)).is_empty());
}

fn disc10_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x12, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [7, 12, 14, 1, 1, 1]),
        record(14, 0x10, [7, 13, 1, 1, 1, 1]),
        record(20, 0x16, [100, 30, 1, 1, 1, 1]),
        record(21, 0x16, [101, 31, 1, 1, 1, 1]),
        record(22, 0x16, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1a, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1a, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc10_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc10_terminal_lattice();
    let bodies = disc20_disc12_disc1e_disc1c_disc10_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc12-disc1e-disc1c-disc10 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc10_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc10_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc20_disc12_disc1e_disc1c_disc10_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc10_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc10_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc20_disc12_disc1e_disc1c_disc10_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc10_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc10_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc20_disc12_disc1e_disc1c_disc10_face_root_body(&index_records(&records)).is_empty());
}

fn disc04_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x12, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(22, 0x0e, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1a, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1a, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc04_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc04_terminal_lattice();
    let bodies = disc20_disc12_disc1e_disc1c_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc12-disc1e-disc1c-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc04_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc04_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc20_disc12_disc1e_disc1c_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc04_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc04_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc20_disc12_disc1e_disc1c_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc04_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc04_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc20_disc12_disc1e_disc1c_disc04_face_root_body(&index_records(&records)).is_empty());
}

fn disc14_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x20, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x12, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [7, 12, 14, 1, 1, 1]),
        record(14, 0x14, [7, 13, 1, 1, 1, 1]),
        record(20, 0x10, [100, 30, 1, 1, 1, 1]),
        record(21, 0x10, [101, 31, 1, 1, 1, 1]),
        record(22, 0x10, [102, 50, 1, 1, 1, 1]),
        record(30, 0x16, [100, 40, 20, 1, 1, 1]),
        record(31, 0x16, [101, 41, 1, 1, 1, 1]),
        record(32, 0x16, [102, 42, 22, 1, 1, 1]),
        record(33, 0x16, [103, 43, 1, 1, 1, 1]),
        flo4(40, 0x22, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x22, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x22, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x22, [103, 1, 33, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc14_terminal_lattice_owns_forward_and_keyed_faces_with_extra_records() {
    let records = disc14_terminal_lattice();
    let bodies = disc20_disc12_disc1e_disc1c_disc14_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc12-disc1e-disc1c-disc14 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&43));
}

#[test]
fn disc14_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc14_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc20_disc12_disc1e_disc1c_disc14_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc14_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc14_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc20_disc12_disc1e_disc1c_disc14_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc14_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc14_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;

    assert!(disc20_disc12_disc1e_disc1c_disc14_face_root_body(&index_records(&records)).is_empty());
}

fn disc10_reordered_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x16, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x12, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x14, [7, 12, 14, 1, 1, 1]),
        record(14, 0x10, [7, 13, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(22, 0x04, [102, 50, 1, 1, 1, 1]),
        record(30, 0x18, [100, 40, 20, 1, 1, 1]),
        record(31, 0x18, [101, 41, 1, 1, 1, 1]),
        record(32, 0x18, [102, 42, 22, 1, 1, 1]),
        record(33, 0x18, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x1c, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1c, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x1c, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc10_reordered_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc10_reordered_terminal_lattice();
    let bodies = disc16_disc12_disc1a_disc14_disc10_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc16-disc12-disc1a-disc14-disc10 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc10_reordered_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc10_reordered_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc16_disc12_disc1a_disc14_disc10_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc10_reordered_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc10_reordered_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc16_disc12_disc1a_disc14_disc10_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc10_reordered_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc10_reordered_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc16_disc12_disc1a_disc14_disc10_face_root_body(&index_records(&records)).is_empty());
}

fn disc04_reordered_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1e, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x12, [100, 30, 1, 1, 1, 1]),
        record(21, 0x12, [101, 31, 1, 1, 1, 1]),
        record(22, 0x12, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1c, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1c, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x24, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc04_reordered_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc04_reordered_terminal_lattice();
    let bodies = disc22_disc1a_disc20_disc1e_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc1a-disc20-disc1e-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc04_reordered_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc04_reordered_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc22_disc1a_disc20_disc1e_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc04_reordered_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc04_reordered_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc22_disc1a_disc20_disc1e_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc04_reordered_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc04_reordered_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc22_disc1a_disc20_disc1e_disc04_face_root_body(&index_records(&records)).is_empty());
}

fn disc04_intermediate_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1e, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x10, [100, 30, 1, 1, 1, 1]),
        record(21, 0x10, [101, 31, 1, 1, 1, 1]),
        record(22, 0x10, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1c, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1c, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x24, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc04_intermediate_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc04_intermediate_terminal_lattice();
    let bodies = disc22_disc18_disc20_disc1e_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc18-disc20-disc1e-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc04_intermediate_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc04_intermediate_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc22_disc18_disc20_disc1e_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc04_intermediate_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc04_intermediate_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc22_disc18_disc20_disc1e_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc04_intermediate_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc04_intermediate_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc22_disc18_disc20_disc1e_disc04_face_root_body(&index_records(&records)).is_empty());
}

fn disc04_disc16_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x16, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x20, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1e, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x1a, [100, 30, 1, 1, 1, 1]),
        record(21, 0x1a, [101, 31, 1, 1, 1, 1]),
        record(22, 0x1a, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1c, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1c, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x24, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x24, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x24, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc04_disc16_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc04_disc16_terminal_lattice();
    let bodies = disc22_disc16_disc20_disc1e_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc16-disc20-disc1e-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc04_disc16_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc04_disc16_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc22_disc16_disc20_disc1e_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc04_disc16_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc04_disc16_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc22_disc16_disc20_disc1e_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc04_disc16_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc04_disc16_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc22_disc16_disc20_disc1e_disc04_face_root_body(&index_records(&records)).is_empty());
}

fn disc18_disc22_disc16_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x22, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x16, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1c, [7, 12, 14, 1, 1, 1]),
        record(14, 0x18, [7, 13, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(22, 0x0e, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1a, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1a, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1a, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1a, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x20, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x20, [103, 1, 33, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc18_disc22_disc16_terminal_lattice_owns_forward_and_keyed_faces_with_extra_records() {
    let records = disc18_disc22_disc16_terminal_lattice();
    let bodies = disc22_disc16_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc22-disc16-disc1e-disc1c-disc18 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&43));
}

#[test]
fn disc18_disc22_disc16_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc18_disc22_disc16_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc22_disc16_disc1e_disc1c_disc18_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc18_disc22_disc16_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc18_disc22_disc16_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc22_disc16_disc1e_disc1c_disc18_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc18_disc22_disc16_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc18_disc22_disc16_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc22_disc16_disc1e_disc1c_disc18_face_root_body(&index_records(&records)).is_empty());
}

fn disc04_mixed_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1a, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x0e, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [7, 12, 14, 1, 1, 1]),
        record(14, 0x04, [7, 13, 1, 1, 1, 1]),
        record(20, 0x10, [100, 30, 1, 1, 1, 1]),
        record(21, 0x10, [101, 31, 1, 1, 1, 1]),
        record(22, 0x10, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1c, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1c, [103, 1, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x20, [102, 1, 32, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc04_mixed_terminal_lattice_owns_forward_and_keyed_faces_with_extra_companions() {
    let records = disc04_mixed_terminal_lattice();
    let bodies = disc1a_disc0e_disc1e_disc18_disc04_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1a-disc0e-disc1e-disc18-disc04 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&42));
}

#[test]
fn disc04_mixed_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc04_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc1a_disc0e_disc1e_disc18_disc04_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc04_mixed_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc04_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc1a_disc0e_disc1e_disc18_disc04_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc04_mixed_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc04_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc1a_disc0e_disc1e_disc18_disc04_face_root_body(&index_records(&records)).is_empty());
}

fn disc14_mixed_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1a, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x16, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1e, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [7, 12, 14, 1, 1, 1]),
        record(14, 0x14, [7, 13, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(22, 0x04, [102, 50, 1, 1, 1, 1]),
        record(30, 0x1c, [100, 40, 20, 1, 1, 1]),
        record(31, 0x1c, [101, 41, 1, 1, 1, 1]),
        record(32, 0x1c, [102, 42, 22, 1, 1, 1]),
        record(33, 0x1c, [103, 43, 1, 1, 1, 1]),
        flo4(40, 0x20, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x20, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x20, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x20, [103, 1, 33, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc14_mixed_terminal_lattice_owns_forward_and_keyed_faces_with_extra_records() {
    let records = disc14_mixed_terminal_lattice();
    let bodies = disc1a_disc16_disc1e_disc18_disc14_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1a-disc16-disc1e-disc18-disc14 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&43));
}

#[test]
fn disc14_mixed_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc14_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc1a_disc16_disc1e_disc18_disc14_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc14_mixed_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc14_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc1a_disc16_disc1e_disc18_disc14_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc14_mixed_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc14_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;

    assert!(disc1a_disc16_disc1e_disc18_disc14_face_root_body(&index_records(&records)).is_empty());
}

fn disc10_mixed_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1c, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x14, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [7, 12, 14, 1, 1, 1]),
        record(14, 0x10, [7, 13, 1, 1, 1, 1]),
        record(20, 0x04, [100, 30, 1, 1, 1, 1]),
        record(21, 0x04, [101, 31, 1, 1, 1, 1]),
        record(22, 0x04, [102, 50, 1, 1, 1, 1]),
        record(30, 0x16, [100, 40, 20, 1, 1, 1]),
        record(31, 0x16, [101, 41, 1, 1, 1, 1]),
        record(32, 0x16, [102, 42, 22, 1, 1, 1]),
        record(33, 0x16, [103, 43, 1, 1, 1, 1]),
        flo4(40, 0x1e, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x1e, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x1e, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x1e, [103, 1, 33, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc10_mixed_terminal_lattice_owns_forward_and_keyed_faces_with_extra_records() {
    let records = disc10_mixed_terminal_lattice();
    let bodies = disc1c_disc14_disc1a_disc18_disc10_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc14-disc1a-disc18-disc10 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&43));
}

#[test]
fn disc10_mixed_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc10_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc1c_disc14_disc1a_disc18_disc10_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc10_mixed_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc10_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc1c_disc14_disc1a_disc18_disc10_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc10_mixed_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc10_mixed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;

    assert!(disc1c_disc14_disc1a_disc18_disc10_face_root_body(&index_records(&records)).is_empty());
}

fn disc18_keyed_terminal_lattice() -> Vec<super::super::EntityRecord> {
    vec![
        flo2(10, 0x1c, [7, 1, 11, 1, 1, 1]),
        flo2(11, 0x16, [7, 10, 12, 1, 1, 1]),
        flo2(12, 0x24, [7, 11, 13, 1, 1, 1]),
        flo2(13, 0x1a, [7, 12, 14, 1, 1, 1]),
        record(14, 0x18, [7, 13, 1, 1, 1, 1]),
        record(20, 0x0e, [100, 30, 1, 1, 1, 1]),
        record(21, 0x0e, [101, 31, 1, 1, 1, 1]),
        record(22, 0x0e, [102, 50, 1, 1, 1, 1]),
        record(30, 0x22, [100, 40, 20, 1, 1, 1]),
        record(31, 0x22, [101, 41, 1, 1, 1, 1]),
        record(32, 0x22, [102, 42, 22, 1, 1, 1]),
        record(33, 0x22, [103, 43, 1, 1, 1, 1]),
        flo4(40, 0x26, [100, 1, 30, 1, 1, 1]),
        flo4(41, 0x26, [101, 1, 1, 1, 1, 1]),
        flo4(42, 0x26, [102, 1, 32, 1, 1, 1]),
        flo4(43, 0x26, [103, 1, 33, 1, 1, 1]),
        record(50, 0x02, [1, 1, 1, 1, 1, 1]),
    ]
}

#[test]
fn disc18_keyed_terminal_lattice_owns_forward_and_keyed_faces_with_extra_records() {
    let records = disc18_keyed_terminal_lattice();
    let bodies = disc1c_disc16_disc24_disc1a_disc18_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc16-disc24-disc1a-disc18 body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 14);
    assert!(body.refs.contains(&20) && body.refs.contains(&21) && body.refs.contains(&22));
    assert!(body.refs.contains(&30) && body.refs.contains(&31) && body.refs.contains(&32));
    assert!(body.refs.contains(&33) && body.refs.contains(&40) && body.refs.contains(&43));
}

#[test]
fn disc18_keyed_terminal_lattice_accepts_stale_reverse_links_by_key() {
    let mut records = disc18_keyed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
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

    let bodies = disc1c_disc16_disc24_disc1a_disc18_face_root_body(&index_records(&records));
    assert_eq!(bodies.len(), 1);
}

#[test]
fn disc18_keyed_terminal_lattice_rejects_a_companion_with_a_different_key() {
    let mut records = disc18_keyed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 31)
        .expect("second companion")
        .refs[0] = 102;

    assert!(disc1c_disc16_disc24_disc1a_disc18_face_root_body(&index_records(&records)).is_empty());
}

#[test]
fn disc18_keyed_terminal_lattice_rejects_ambiguous_same_key_companions() {
    let mut records = disc18_keyed_terminal_lattice();
    records
        .iter_mut()
        .find(|record| record.attr == 20)
        .expect("first canonical face")
        .refs[1] = 50;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("unselected companion")
        .refs[0] = 100;
    records
        .iter_mut()
        .find(|record| record.attr == 33)
        .expect("ambiguous companion")
        .refs[1] = 43;

    assert!(disc1c_disc16_disc24_disc1a_disc18_face_root_body(&index_records(&records)).is_empty());
}
