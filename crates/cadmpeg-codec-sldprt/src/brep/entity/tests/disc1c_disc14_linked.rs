use super::super::disc1c_disc14_disc0e_linked_face_root_body;
use super::{flo2, flo4, index_records, record};

#[test]
fn disc0c_faces_bind_direct_and_intermediate_face_uses() {
    let records = [
        flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
        record(13, 0x14, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
        record(20, 0x0c, [1, 30, 1, 1, 1, 1]),
        record(21, 0x0c, [1, 31, 1, 1, 1, 1]),
        record(30, 0x0e, [1, 40, 20, 1, 1, 1]),
        record(31, 0x0e, [1, 41, 21, 1, 1, 1]),
        record(40, 0x16, [1, 50, 30, 1, 1, 1]),
        flo2(41, 0x10, [1, 42, 31, 1, 1, 1]),
        record(42, 0x16, [1, 51, 41, 1, 1, 1]),
        flo4(50, 0x1e, [1, 1, 40, 1, 1, 1]),
        flo4(51, 0x1e, [1, 1, 42, 1, 1, 1]),
    ];
    let by_attr = index_records(&records);

    let bodies = disc1c_disc14_disc0e_linked_face_root_body(&by_attr);
    let [body] = bodies.as_slice() else {
        panic!("one disc1c-disc14-disc0e-linked-face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 12);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));

    let mut broken_intermediate = records;
    broken_intermediate[11].refs[2] = 1;
    assert!(
        disc1c_disc14_disc0e_linked_face_root_body(&index_records(&broken_intermediate)).is_empty()
    );
}
