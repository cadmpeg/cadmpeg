use super::super::disc1c_disc16_disc0e_face_root_body;
use super::{flo2, flo4, index_records, record};

#[test]
fn lattice_requires_reciprocal_links() {
    let records = [
        flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
        record(13, 0x12, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [3, 14, 1, 1, 1, 1]),
        record(20, 0x04, [1, 30, 1, 1, 1, 1]),
        record(21, 0x04, [1, 31, 1, 1, 1, 1]),
        record(30, 0x14, [1, 40, 20, 1, 1, 1]),
        record(31, 0x14, [1, 41, 21, 1, 1, 1]),
        flo4(40, 0x1a, [1, 1, 30, 1, 1, 1]),
        flo4(41, 0x1a, [1, 1, 31, 1, 1, 1]),
    ];
    let by_attr = index_records(&records);

    let bodies = disc1c_disc16_disc0e_face_root_body(&by_attr);
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc16-disc0e-face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));

    let mut broken_face_link = records.clone();
    broken_face_link[8].refs[2] = 1;
    assert!(disc1c_disc16_disc0e_face_root_body(&index_records(&broken_face_link)).is_empty());

    let mut broken_terminal_sentinel = records.clone();
    broken_terminal_sentinel[5].refs[2] = 0;
    assert!(
        disc1c_disc16_disc0e_face_root_body(&index_records(&broken_terminal_sentinel)).is_empty()
    );

    let mut broken_use_link = records;
    broken_use_link[10].refs[2] = 1;
    assert!(disc1c_disc16_disc0e_face_root_body(&index_records(&broken_use_link)).is_empty());
}

#[test]
fn disc04_terminal_lattice_owns_disc0c_faces() {
    let records = [
        flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
        record(13, 0x12, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x0c, [1, 30, 1, 1, 1, 1]),
        record(21, 0x0c, [1, 31, 1, 1, 1, 1]),
        record(30, 0x14, [1, 40, 20, 1, 1, 1]),
        record(31, 0x14, [1, 41, 21, 1, 1, 1]),
        flo4(40, 0x1a, [1, 1, 30, 1, 1, 1]),
        flo4(41, 0x1a, [1, 1, 31, 1, 1, 1]),
    ];
    let by_attr = index_records(&records);

    let bodies = super::super::disc1c_disc16_disc0e_disc04_face_root_body(&by_attr);
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc16-disc0e-disc04-face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));

    let mut broken_terminal = records;
    broken_terminal[6].refs[2] = 0;
    assert!(
        super::super::disc1c_disc16_disc0e_disc04_face_root_body(&index_records(&broken_terminal))
            .is_empty()
    );
}

#[test]
fn disc12_disc0e_terminal_lattice_owns_disc04_faces() {
    let records = [
        flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x12, [3, 12, 14, 1, 1, 1]),
        record(14, 0x10, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x0e, [3, 14, 1, 1, 1, 1]),
        record(20, 0x04, [1, 30, 1, 1, 1, 1]),
        record(21, 0x04, [1, 31, 1, 1, 1, 1]),
        record(30, 0x14, [1, 40, 20, 1, 1, 1]),
        record(31, 0x14, [1, 41, 21, 1, 1, 1]),
        flo4(40, 0x1a, [1, 1, 30, 1, 1, 1]),
        flo4(41, 0x1a, [1, 1, 31, 1, 1, 1]),
    ];
    let by_attr = index_records(&records);

    let bodies = super::super::disc1c_disc16_disc12_disc0e_face_root_body(&by_attr);
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc16-disc12-disc0e-face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));

    let mut broken_chain = records;
    broken_chain[5].refs[2] = 0;
    assert!(
        super::super::disc1c_disc16_disc12_disc0e_face_root_body(&index_records(&broken_chain))
            .is_empty()
    );
}
