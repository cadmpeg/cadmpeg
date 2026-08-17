use super::super::disc1e_disc04_terminal_face_root_body;
use super::{flo2, flo4, index_records, record};

#[test]
fn disc1e_disc04_terminal_face_root_accepts_direct_close() {
    let records = vec![
        flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
        record(13, 0x16, [3, 12, 14, 1, 1, 1]),
        flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
        record(20, 0x0e, [1; 6]),
        record(21, 0x0e, [1; 6]),
        record(30, 0x18, [1; 6]),
        record(31, 0x18, [1; 6]),
        flo4(40, 0x20, [1; 6]),
        flo4(41, 0x20, [1; 6]),
    ];

    let bodies = disc1e_disc04_terminal_face_root_body(&index_records(&records));
    let [body] = bodies.as_slice() else {
        panic!("one disc1e-disc04-terminal-face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
}
