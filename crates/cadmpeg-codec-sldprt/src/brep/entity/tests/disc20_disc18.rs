use super::super::disc20_disc18_disc14_face_root_body;
use super::{flo2, flo4, index_records, record};

#[test]
fn disc20_disc18_disc14_face_root_lattice_owns_the_site() {
    let records = vec![
        flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
        flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
        flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
        flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
        record(14, 0x14, [3, 13, 15, 1, 1, 1]),
        flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
        flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
        record(20, 0x0e, [1; 6]),
        record(21, 0x0e, [1; 6]),
        record(30, 0x1a, [1; 6]),
        record(31, 0x1a, [1; 6]),
        flo4(40, 0x22, [1; 6]),
        flo4(41, 0x22, [1; 6]),
    ];

    let bodies = disc20_disc18_disc14_face_root_body(&index_records(&records), &records);
    let [body] = bodies.as_slice() else {
        panic!("one disc20-disc18-disc14-face-root body");
    };
    assert_eq!(body.attr, 10);
    assert_eq!(body.regions[0].shells[0].attr, 13);
    assert!(body.refs.contains(&20) && body.refs.contains(&21));
    assert!(body.refs.contains(&40) && body.refs.contains(&41));

    let direct_records = records
        .iter()
        .filter(|record| record.attr != 15)
        .map(|record| {
            let mut record = record.clone();
            if record.attr == 14 {
                record.refs[2] = 16;
            }
            record
        })
        .collect::<Vec<_>>();
    let direct_bodies =
        disc20_disc18_disc14_face_root_body(&index_records(&direct_records), &direct_records);
    assert_eq!(direct_bodies.len(), 1);

    let via_disc12 = records
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if record.attr == 15 {
                record.disc = 0x12;
            }
            record
        })
        .collect::<Vec<_>>();
    let via_disc12_bodies =
        disc20_disc18_disc14_face_root_body(&index_records(&via_disc12), &via_disc12);
    assert_eq!(via_disc12_bodies.len(), 1);

    let missing_use = records
        .iter()
        .filter(|record| record.attr != 41)
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        disc20_disc18_disc14_face_root_body(&index_records(&missing_use), &missing_use).is_empty()
    );
}
