// SPDX-License-Identifier: Apache-2.0
//! Synthetic `sgSketch` record lists (`docs/formats/sldprt.md`, "Sketch
//! objects").

/// The 64-byte head of an `sgSketch` record (`docs/formats/sldprt.md`,
/// "Sketch objects").
pub(crate) fn sketch_record_head(out: &mut Vec<u8>) {
    out.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    out.extend_from_slice(&[0xff; 8]);
    out.extend_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0x04, 0x00, 0x02, 0x00]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&1.0f64.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

/// A lane payload holding one `sgSketch` object with point records at
/// `points` (metres) and curve records `(start, end, selector)` that are
/// lines. Returns the payload and the record offsets.
pub(crate) fn framed_line_sketch(
    points: &[[f64; 2]],
    lines: &[(u16, u16, u16)],
) -> (Vec<u8>, Vec<u64>, Vec<u64>) {
    let offset = |payload: &Vec<u8>| u64::try_from(payload.len()).unwrap();
    let mut payload = vec![0; 7];
    payload.extend_from_slice(&u32::try_from(points.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    payload.extend_from_slice(&1u32.to_le_bytes());
    let mut point_offsets = Vec::new();
    for [u, v] in points {
        point_offsets.push(offset(&payload));
        sketch_record_head(&mut payload);
        payload.extend_from_slice(&0x1eu16.to_le_bytes());
        payload.extend_from_slice(&u.to_le_bytes());
        payload.extend_from_slice(&v.to_le_bytes());
        payload.extend_from_slice(&[0; 10]);
        payload.extend_from_slice(&(-2i32).to_le_bytes());
        payload.extend_from_slice(&[0; 46]);
    }
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut line_offsets = Vec::new();
    for &(start, end, selector) in lines {
        let at = payload.len();
        line_offsets.push(offset(&payload));
        sketch_record_head(&mut payload);
        payload[at + 37..at + 39].copy_from_slice(&selector.to_le_bytes());
        payload.extend_from_slice(&start.to_le_bytes());
        payload.extend_from_slice(&end.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&(-1.0f64).to_le_bytes());
        payload.extend_from_slice(&0i32.to_le_bytes());
        payload.extend_from_slice(&[0; 12]);
    }
    (payload, point_offsets, line_offsets)
}
