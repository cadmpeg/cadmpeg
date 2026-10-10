// SPDX-License-Identifier: Apache-2.0

#[test]
fn named_row_scan_keeps_first_fields_and_stops_at_the_next_namespace() {
    let mut payload = b"padding".to_vec();
    let start = payload.len();
    let mut expected = [None; 6];
    for field in [2, 5, 0, 4, 1, 3] {
        expected[field] = Some(payload.len());
        payload.extend_from_slice(crate::surface::NAMED_ROW_FIELDS[field]);
        payload.push(7);
    }
    payload.extend_from_slice(b"geom_id\0\x09");
    let end = payload.len();
    payload.extend_from_slice(b"srf_array\0geom_id\0\x0b");
    let actual = super::work_output(|ctx| crate::surface::named_row_fields(ctx, &payload, start));
    assert_eq!(actual, (expected, end));
}

#[test]
fn surface_frame_scan_selects_each_earliest_array_boundary() {
    for label in [b"srf_array\0".as_slice(), b"crv_array\0", b"lo_array\0", b"qlt_array\0"] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        let start = payload.len();
        payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
        let end = payload.len();
        payload.extend_from_slice(label);
        payload.extend_from_slice(b"paddinglo_array\0srf_array\0");
        let frame = super::work_output(|ctx| {
            crate::surface::next_surface_array_frame(ctx, &payload, &mut 0)
        }).expect("complete frame header");
        assert_eq!(frame.start, start);
        assert_eq!(frame.end, end);
        assert_eq!(frame.count, 1);
    }
}
