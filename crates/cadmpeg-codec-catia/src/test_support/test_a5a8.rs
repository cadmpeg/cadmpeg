// SPDX-License-Identifier: Apache-2.0
//! a5/a6/a8-family synthetic stream and CATPart builders.

#![allow(clippy::unwrap_used)]
use super::{compact_uint_bytes, le_f64, object_main_catpart};

pub(crate) fn a8_surface_stream() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(0); // lead
    payload.extend_from_slice(&[9, 0, 0, 9, 1]); // degree, flags, K, marker
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.extend_from_slice(&[13, 13]); // multiplicities [3, 3]
    payload.extend_from_slice(&[9, 0, 0, 9, 1]);
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.extend_from_slice(&[13, 13, 1]); // multiplicities and plain mode
    for i in 0..9 {
        for value in [i as f64, (i / 3) as f64, (i % 3) as f64] {
            payload.extend_from_slice(&le_f64(value));
        }
    }
    let mut record = Vec::new();
    record.extend_from_slice(&[0xa8, 0x03, 0x34]);
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0xdeca_fbad_u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a8_surface_stream_with_u_count(u_count: u32) -> Vec<u8> {
    assert!(u_count >= 2);
    let mut payload = vec![0, 9, 0, 0];
    payload.extend_from_slice(&compact_uint_bytes(u_count));
    payload.push(1);
    for knot in 0..u_count {
        payload.extend_from_slice(&le_f64(f64::from(knot)));
    }
    for knot in 0..u_count {
        let multiplicity = if knot == 0 || knot + 1 == u_count {
            3
        } else {
            1
        };
        payload.extend_from_slice(&compact_uint_bytes(multiplicity));
    }
    payload.extend_from_slice(&[9, 0, 0, 9, 1]);
    payload.extend_from_slice(&[le_f64(0.0), le_f64(1.0)].concat());
    payload.extend_from_slice(&[13, 13, 1]);
    let u_poles = u_count + 1;
    for pole in 0..u_poles * 3 {
        payload.extend_from_slice(&le_f64(f64::from(pole)));
        payload.extend_from_slice(&le_f64(0.0));
        payload.extend_from_slice(&le_f64(0.0));
    }
    let mut record = vec![0xa8, 0x03, 0x34];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0xdeca_fbad_u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a8_surface_tail() -> Vec<u8> {
    let mut tail = vec![0; 141];
    tail[..4].copy_from_slice(&[0x05, 0x21, 0x05, 0x05]);
    for (offset, value) in [
        (4, 0.0),
        (12, 1.0),
        (20, 0.0),
        (28, 1.0),
        (36, 1.0),
        (44, 0.0),
        (52, 1.0),
        (60, 0.0),
    ] {
        tail[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    tail[68..71].copy_from_slice(&[0x01, 0x01, 0x01]);
    tail[135..141].copy_from_slice(&[0x01, 0x00, 0x01, 0x00, 0x07, 0x07]);
    tail
}

pub(crate) fn a8_inline_tail_surface_stream() -> Vec<u8> {
    let mut bytes = a8_surface_stream();
    bytes.extend_from_slice(&a8_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());
    bytes
}

pub(crate) fn a8_elided_surface_stream() -> Vec<u8> {
    const SURFACE: u32 = 100;

    let mut bytes = a8_surface_stream();
    bytes.truncate(59);
    bytes[7..11].copy_from_slice(&SURFACE.to_le_bytes());
    bytes.extend_from_slice(&a8_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let mut pcurve_payload = vec![0; 58];
    pcurve_payload[0] = 0x81;
    pcurve_payload[1] = 0x18;
    pcurve_payload[2..4].copy_from_slice(&(SURFACE as u16).to_le_bytes());
    pcurve_payload[57] = 0x07;
    bytes.extend_from_slice(&[0xb5, 0x03, 0x21, 58, 1, 0, 0, 0]);
    bytes.extend_from_slice(&pcurve_payload);
    for point in 0..9 {
        for coordinate in [f64::from(point), f64::from(point % 3), 2.0] {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0, 2, 0, 0, 0, 0, 0]);
    bytes
}

pub(crate) fn a8_rational_surface_stream() -> Vec<u8> {
    let mut record = a8_surface_stream();
    // Header is 11 bytes; the common-form mode follows the two degree/knot
    // sections at record offset 58 for this 2×2 distinct-knot fixture.
    record[58] = 0x05;
    for _ in 0..9 {
        record.extend_from_slice(&le_f64(2.0));
    }
    let payload_len = (record.len() - 11) as u32;
    record[3..7].copy_from_slice(&payload_len.to_le_bytes());
    record
}

pub(crate) fn a8_pcurve_stream() -> Vec<u8> {
    let mut payload = vec![0, 0x18, 0x34, 0x12, 21, 0, 0, 9, 0x0c];
    for value in [0.0f64, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[25, 25, 9, 1]);
    for values in [[0.0f64, 1.0], [0.0, 1.0], [1.0, 1.0], [0.0, 0.0]] {
        for value in values {
            payload.extend_from_slice(&le_f64(value));
        }
    }
    payload.push(0x05);
    for _ in 0..4 {
        payload.extend_from_slice(&le_f64(0.0));
    }
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.push(0x07);
    let mut record = vec![0xa8, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0x5678u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a8_pcurve_stream_with_count(count: u32) -> Vec<u8> {
    assert!(count >= 2);
    let mut payload = vec![0, 0x18, 0x34, 0x12, 21, 0, 0];
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(0x0c);
    for site in 0..count {
        payload.extend_from_slice(&le_f64(f64::from(site)));
    }
    for site in 0..count {
        let multiplicity = if site == 0 || site + 1 == count { 6 } else { 3 };
        payload.extend_from_slice(&compact_uint_bytes(multiplicity));
    }
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(1);
    for array in 0..4 {
        for _ in 0..count {
            let value = if array == 2 { 1.0 } else { 0.0 };
            payload.extend_from_slice(&le_f64(value));
        }
    }
    payload.push(0x05);
    for _ in 0..count {
        payload.extend_from_slice(&le_f64(0.0));
    }
    for _ in 0..count {
        payload.extend_from_slice(&le_f64(0.0));
    }
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(f64::from(count - 1)));
    payload.push(0x07);
    let mut record = vec![0xa8, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0x5678u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a5_pcurve_stream() -> Vec<u8> {
    a5_pcurve_stream_with_uv([0.0, 1.0], [0.0, 1.0])
}

pub(crate) fn a5_pcurve_stream_with_count(count: u32) -> Vec<u8> {
    assert!(count >= 2);
    let mut payload = vec![0x08, 0x34, 0x12, 21];
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.extend_from_slice(&[0x08, 9]);
    for site in 0..count {
        payload.extend_from_slice(&le_f64(f64::from(site)));
    }
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(2);
    for array in 0..4 {
        for _ in 0..count {
            let value = if array == 2 { 1.0 } else { 0.0 };
            payload.extend_from_slice(&le_f64(value));
        }
    }
    payload.push(0x05);
    for _ in 0..count {
        payload.extend_from_slice(&le_f64(0.0));
    }
    for _ in 0..count {
        payload.extend_from_slice(&le_f64(0.0));
    }
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(f64::from(count - 1)));
    payload.push(0x07);
    let mut record = vec![0xa5, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a6_pcurve_stream() -> Vec<u8> {
    let narrow = a5_pcurve_stream();
    let mut wide = vec![0xa6, 0x03, 0x20];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

pub(crate) fn a5_pcurve_stream_with_uv(u: [f64; 2], v: [f64; 2]) -> Vec<u8> {
    a5_pcurve_stream_with_support_and_uv(0x1234, u, v)
}

pub(crate) fn a5_pcurve_stream_with_support_and_uv(
    support_id: u32,
    u: [f64; 2],
    v: [f64; 2],
) -> Vec<u8> {
    let mut payload = compact_uint_bytes(support_id);
    payload.extend_from_slice(&[21, 9, 0x08, 9]);
    for value in [0.0f64, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[9, 2]);
    for values in [u, v, [1.0, 1.0], [0.0, 0.0]] {
        for value in values {
            payload.extend_from_slice(&le_f64(value));
        }
    }
    payload.push(0x05);
    for _ in 0..4 {
        payload.extend_from_slice(&le_f64(0.0));
    }
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.push(0x07);
    let mut record = vec![0xa5, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a5_native_edge_identity_stream(curve: u8, start: u8, end: u8) -> Vec<u8> {
    assert!(curve >= 3);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[
        0xb2,
        0x03,
        0x06,
        0x04,
        0x05,
        0x82,
        4 * (curve - 2) + 1,
        4 * (curve - 1) + 1,
        0x88,
    ]);
    bytes.extend_from_slice(&[
        0xb2,
        0x03,
        0x06,
        0x04,
        0x05,
        0x82,
        4 * (curve - 1) + 1,
        4 * curve + 1,
        0x84,
    ]);
    let mut payload = vec![4 * curve + 1, 0x06, start, 0x06, end, 9, 5, 0x21];
    bytes.extend_from_slice(&[0xb2, 0x03, 0x5e, u8::try_from(payload.len()).unwrap(), 0x05]);
    bytes.append(&mut payload);
    bytes
}

pub(crate) fn a5_surface_stream() -> Vec<u8> {
    a5_surface_stream_with_poles([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [2.0, 1.0, 0.0],
        [3.0, 1.0, 1.0],
    ])
}

pub(crate) fn a6_surface_stream() -> Vec<u8> {
    let narrow = a5_surface_stream();
    let mut wide = vec![0xa6, 0x03, 0x34];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

pub(crate) fn a5_surface_stream_with_poles(poles: [[f64; 3]; 4]) -> Vec<u8> {
    a5_surface_record_with_tail(poles, &a5_surface_tail())
}

pub(crate) fn a5_surface_stream_with_tail(tail: &[u8]) -> Vec<u8> {
    a5_surface_record_with_tail(
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [2.0, 1.0, 0.0],
            [3.0, 1.0, 1.0],
        ],
        tail,
    )
}

pub(crate) fn a5_surface_record_with_tail(poles: [[f64; 3]; 4], tail: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&[0xa5, 0x03, 0x34]);
    record.extend_from_slice(&0u32.to_le_bytes());
    record.push(0); // unclassified byte before the compact header
    record.extend_from_slice(&[5, 9, 0x0c]); // degree 1, two U knots
    record.extend_from_slice(&le_f64(0.0));
    record.extend_from_slice(&le_f64(1.0));
    record.extend_from_slice(&[5, 9, 0x0c]); // degree 1, two V knots
    record.extend_from_slice(&le_f64(0.0));
    record.extend_from_slice(&le_f64(1.0));
    record.push(0x01); // non-rational
    for pole in poles {
        for value in pole {
            record.extend_from_slice(&le_f64(value));
        }
    }
    record.extend_from_slice(tail);
    let payload_len = u32::try_from(record.len() - 8).unwrap();
    record[3..7].copy_from_slice(&payload_len.to_le_bytes());
    record
}

pub(crate) fn a5_surface_parameter_tail(
    flags: [u8; 3],
    continuation: &[f64],
    suffix: &[u8],
) -> Vec<u8> {
    let mut tail = vec![0x05, 0x05, 0x05, 0x05];
    for value in [0.0f64, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0] {
        tail.extend_from_slice(&le_f64(value));
    }
    tail.extend_from_slice(&flags);
    for value in continuation {
        tail.extend_from_slice(&le_f64(*value));
    }
    tail.extend_from_slice(suffix);
    tail
}

pub(crate) fn a5_surface_short_tail() -> Vec<u8> {
    a5_surface_parameter_tail(
        [0x01, 0x01, 0x01],
        &[0.0; 7],
        &[0x01, 0x00, 0x01, 0x00, 0x07, 0x07],
    )
}

pub(crate) fn a5_surface_tail() -> Vec<u8> {
    a5_surface_parameter_tail(
        [0x01, 0x01, 0x01],
        &[0.0; 8],
        &[0x01, 0x00, 0x01, 0x00, 0x07, 0x07],
    )
}

pub(crate) fn a5_surface_extrapolated_tail() -> Vec<u8> {
    a5_surface_parameter_tail(
        [0x05, 0x05, 0x01],
        &[0.25, 0.5, 0.25, 0.75, 0.5, 0.75, 0.5, 1.0],
        &[0x09, 0x00, 0x09, 0x01, 0x05, 0x07, 0x07],
    )
}

pub(crate) fn a5_surface_extrapolated_short_tail() -> Vec<u8> {
    a5_surface_parameter_tail(
        [0x05, 0x05, 0x01],
        &[0.25, 0.5, 0.25, 0.75, 0.5, 0.75, 0.5, 1.0],
        &[0x09, 0x00, 0x09, 0x00, 0x07, 0x07],
    )
}

pub(crate) fn a5_rational_surface_stream() -> Vec<u8> {
    let mut record = a5_surface_stream();
    record[46] = 0x05;
    let tail = record.split_off(143);
    record.extend_from_slice(&[0x01, 0x07, 0x00]);
    record.extend_from_slice(&le_f64(2.0)); // mirrored seed row -> [2, 2]
    record.push(0x02); // copy the row for the second u row
    record.extend_from_slice(&tail);
    let payload_len = u32::try_from(record.len() - 8).unwrap();
    record[3..7].copy_from_slice(&payload_len.to_le_bytes());
    record
}

pub(crate) fn a5_freeform_curve_stream() -> Vec<u8> {
    a5_freeform_curve_stream_with_count(2)
}

pub(crate) fn a5_freeform_curve_stream_with_count(count: u32) -> Vec<u8> {
    let mut payload = compact_uint_bytes(count);
    payload.push(21);
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(0x0c);
    for site in 0..count {
        payload.extend_from_slice(&le_f64(f64::from(site)));
    }
    for block in 0..3 {
        for site in 0..count {
            let radius = if site == 0 { 1.0 } else { 2.0 };
            let values = if block == 0 {
                [
                    radius,
                    0.0,
                    0.0,
                    0.0,
                    radius,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    std::f64::consts::FRAC_PI_2,
                ]
            } else {
                [0.0; 10]
            };
            for value in values {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    let mut record = vec![0xa5, 0x03, 0x32];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a6_freeform_curve_stream() -> Vec<u8> {
    let narrow = a5_freeform_curve_stream();
    let mut wide = vec![0xa6, 0x03, 0x32];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

pub(crate) fn a5_guide_curve_stream() -> Vec<u8> {
    a5_guide_curve_stream_with_count(2)
}

pub(crate) fn a5_guide_curve_stream_with_count(count: u32) -> Vec<u8> {
    let mut payload = compact_uint_bytes(count);
    payload.push(21);
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(0x0c);
    for site in 0..count {
        payload.extend_from_slice(&le_f64(f64::from(site)));
    }
    for block in 0..3 {
        for site in 0..count {
            let values = if block == 0 {
                if site == 0 {
                    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]
                } else {
                    [2.0, 3.0, 4.0, 2.0, 4.0, 4.0]
                }
            } else {
                [0.0; 6]
            };
            for value in values {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    payload.extend_from_slice(&[0; 48]);
    let mut record = vec![0xa5, 0x03, 0x39];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a8_freeform_curve_stream() -> Vec<u8> {
    a8_freeform_curve_stream_with_count(2)
}

pub(crate) fn a8_freeform_curve_stream_with_count(count: u32) -> Vec<u8> {
    let mut payload = vec![0];
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.extend_from_slice(&[21, 0, 0]);
    payload.extend_from_slice(&compact_uint_bytes(count));
    payload.push(0x0c);
    for site in 0..count {
        payload.extend_from_slice(&le_f64(f64::from(site)));
    }
    for site in 0..count {
        let multiplicity = if site == 0 || site + 1 == count { 6 } else { 3 };
        payload.extend_from_slice(&compact_uint_bytes(multiplicity));
    }
    let sites = [
        [
            1.0f64,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
        [
            2.0,
            0.0,
            0.0,
            0.0,
            2.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
    ];
    for block in 0..3 {
        for site in 0..count {
            let values = if block == 0 {
                if site == 0 {
                    sites[0]
                } else {
                    sites[1]
                }
            } else {
                [0.0; 10]
            };
            for value in values {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    payload.extend_from_slice(&[0; 59]);
    let mut record = vec![0xa8, 0x03, 0x32];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0x1234_5678u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a8_catpart() -> Vec<u8> {
    object_main_catpart(&a8_surface_stream())
}

pub(crate) fn inner_no_directory_a8_catpart() -> Vec<u8> {
    let mut file = a8_catpart();
    let name = b"M\x00a\x00i\x00n\x00D\x00a\x00t\x00a\x00S\x00t\x00r\x00e\x00a\x00m\x00";
    let pos = file
        .windows(name.len())
        .position(|bytes| bytes == name)
        .expect("main stream name");
    file[pos] = b'X';
    file
}
