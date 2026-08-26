// SPDX-License-Identifier: Apache-2.0
//! b2/b3-family synthetic stream and CATPart builders.

#![allow(clippy::unwrap_used)]
use super::{a5_pcurve_stream, compact_uint_bytes, le_f64, object_main_catpart};

pub(crate) fn b2_pcurve_stream() -> Vec<u8> {
    let narrow = a5_pcurve_stream();
    let payload = &narrow[8..];
    let mut record = vec![0xb2, 0x03, 0x20, u8::try_from(payload.len()).unwrap(), 0x05];
    record.extend_from_slice(payload);
    record
}

pub(crate) fn b2_plane_carrier_stream() -> Vec<u8> {
    let layouts = [
        (0xe4, vec![10.0, 20.0, 1.0, 0.0, 5.0, -2.0, 3.0]),
        (0xc4, vec![10.0, 20.0, 1.0, 0.0, 0.0, 5.0, -2.0, 3.0]),
        (0xec, vec![10.0, 20.0, -2.0, 5.0, -2.0, 3.0]),
    ];
    let mut bytes = Vec::new();
    for (selector, values) in layouts {
        let payload_len = 2 + values.len() * size_of::<f64>();
        bytes.extend_from_slice(&[
            0xb2,
            0x03,
            0x27,
            u8::try_from(payload_len).expect("class-27 fixture payload"),
            0x05,
            0xb4,
            selector,
        ]);
        for value in values {
            bytes.extend_from_slice(&le_f64(value));
        }
    }
    bytes
}

pub(crate) fn b2_parameter_point_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (prefix, values) in [
        (0x05, vec![2.0f64, 3.0]),
        (0x09, vec![11.0, 4.0, 5.0]),
        (0x0d, vec![1.0, 2.0, 3.0, 4.0, 5.0]),
        (0x11, vec![12.0, 6.0, 7.0]),
    ] {
        let length = 2 + 8 * values.len();
        bytes.extend_from_slice(&[
            0xb2,
            0x03,
            0x18,
            u8::try_from(length).unwrap(),
            0x05,
            prefix,
        ]);
        bytes.push(0x12);
        for value in values {
            bytes.extend_from_slice(&le_f64(value));
        }
    }
    bytes
}

pub(crate) fn b2_reference_list_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x37, 0x22, 0x05];
    for value in 0u8..26 {
        record.push(4 * value + 1);
    }
    record.extend_from_slice(&le_f64(1.0));
    record
}

pub(crate) fn b2_owner_packet_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x62, 0x52, 0x05, 0x89];
    for (index, value) in [1000u16, 1, 1001, 2, 1002, 3, 1003, 4, 1004]
        .into_iter()
        .enumerate()
    {
        if index % 2 == 0 {
            record.push(0x0a);
            record.extend_from_slice(&value.to_le_bytes());
        } else {
            record.push(4 * u8::try_from(value).unwrap() + 1);
        }
    }
    record.extend_from_slice(&owner_numeric_tail());
    record
}

pub(crate) fn b2_width_coded_owner_packet_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x62, 0x50, 0x05, 0x89];
    for (index, value) in [216u16, 3, 540, 7, 223, 19, 545, 31, 606]
        .into_iter()
        .enumerate()
    {
        if index % 2 == 0 {
            if u8::try_from(value).is_ok() {
                record.extend_from_slice(&[0x04, u8::try_from(value).unwrap()]);
            } else {
                record.push(0x08);
                record.extend_from_slice(&value.to_le_bytes());
            }
        } else {
            record.push(u8::try_from(value).unwrap());
        }
    }
    record.extend_from_slice(&owner_numeric_tail());
    record
}

pub(crate) fn b2_width_coded_owner_with_allocation_stream() -> (Vec<u8>, [usize; 5], usize) {
    let mut bytes = Vec::new();
    let mut target_positions = [0usize; 5];
    for (index, class) in [0x5d, 0x5e, 0x5d, 0x5e, 0x5e].into_iter().enumerate() {
        target_positions[index] = bytes.len();
        bytes.extend_from_slice(&[0xb2, 0x03, class, 0x00, 0x05]);
    }
    let owner_pos = bytes.len();
    let mut owner = vec![0xb2, 0x03, 0x62, 0, 0x05, 0x89];
    for distance in [1u8, 4, 2, 3, 5] {
        owner.push(4 * distance + 1);
        owner.push(0);
    }
    owner.pop();
    owner.extend_from_slice(&owner_numeric_tail());
    owner[3] = u8::try_from(owner.len() - 5).expect("fixed owner packet length");
    bytes.extend_from_slice(&owner);
    (bytes, target_positions, owner_pos)
}

pub(crate) fn b2_fixed_owner_boundary_cycle_stream() -> (Vec<u8>, [usize; 4], usize, [[usize; 2]; 4])
{
    let mut bytes = Vec::new();
    let mut endpoint_positions = [0usize; 4];
    for position in &mut endpoint_positions {
        *position = bytes.len();
        bytes.extend_from_slice(&[0xb2, 0x03, 0x5d, 0x02, 0x05, 0x03, 0x00]);
    }

    let endpoint_indices = [[0usize, 1], [0, 2], [2, 3], [1, 3]];
    let mut edge_positions = [0usize; 4];
    for (edge_index, indices) in endpoint_indices.into_iter().enumerate() {
        let current_ordinal = 4 + edge_index;
        let distances = indices
            .map(|index| u8::try_from(current_ordinal - index).expect("cycle endpoint distance"));
        edge_positions[edge_index] = bytes.len();
        let mut edge = vec![
            0xb2,
            0x03,
            0x5e,
            0x09,
            0x05,
            0x06,
            0,
            4 * distances[0] + 1,
            4 * distances[1] + 1,
            0x06,
            0,
            0x06,
            0,
            0x21,
        ];
        bytes.append(&mut edge);
    }

    let owner_pos = bytes.len();
    let mut owner = vec![0xb2, 0x03, 0x62, 0, 0x05, 0x89];
    for (index, distance) in [1000u16, 4, 1001, 3, 1002, 2, 1003, 1, 1004]
        .into_iter()
        .enumerate()
    {
        if index % 2 == 0 {
            owner.push(0x0a);
            owner.extend_from_slice(&distance.to_le_bytes());
        } else {
            owner.push(4 * u8::try_from(distance).expect("cycle distance") + 1);
        }
    }
    owner.extend_from_slice(&owner_numeric_tail());
    owner[3] = u8::try_from(owner.len() - 5).expect("fixed owner packet length");
    bytes.extend_from_slice(&owner);

    (
        bytes,
        edge_positions,
        owner_pos,
        endpoint_indices.map(|indices| {
            [
                endpoint_positions[indices[0]],
                endpoint_positions[indices[1]],
            ]
        }),
    )
}

pub(crate) fn b2_fixed_owner_boundary_face_node_cycle_stream(
) -> (Vec<u8>, [usize; 4], usize, [[usize; 2]; 4], usize) {
    let (mut bytes, mut edge_positions, mut owner_pos, endpoint_records) =
        b2_fixed_owner_boundary_cycle_stream();
    let node_pos = edge_positions[0];
    let node = [
        0xb4, 0x03, 0x5f, 0x06, 0x08, 0x2e, 0x0a, 0x82, 0x0a, 0xf6, 0x03, 0x27, 0x05,
    ];
    bytes.splice(node_pos..node_pos, node);
    for edge in &mut edge_positions {
        *edge += node.len();
    }
    owner_pos += node.len();
    (bytes, edge_positions, owner_pos, endpoint_records, node_pos)
}

pub(crate) fn b2_all_compact_owner_packet_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x62, 0, 0x05, 0x89];
    for value in [278, 324, 276, 268, 277, 374, 199, 195, 279] {
        record.extend_from_slice(&compact_uint_bytes(value));
    }
    record.extend_from_slice(&owner_numeric_tail());
    record[3] = u8::try_from(record.len() - 5).expect("all-compact owner packet length");
    record
}

pub(crate) fn owner_numeric_tail() -> Vec<u8> {
    owner_numeric_tail_for([-0.0, 4.5], [12.25, 7.0])
}

fn owner_numeric_tail_for(lower: [f64; 2], upper: [f64; 2]) -> Vec<u8> {
    let mut tail = vec![0x84, 0x41, 0xbb, 0x05, 0x0d];
    for value in [lower[0], lower[1], upper[0], upper[1]] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail.push(0x01);
    for value in [-2.0f32, 1.0, 3.5, 4.0, 5.25, 6.0] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail
}

pub(crate) fn b2_owner_chart_stream(carrier_class: u8) -> Vec<u8> {
    b2_owner_chart_stream_with_encoding(carrier_class, false)
}

pub(crate) fn b2_width_coded_owner_chart_stream(carrier_class: u8) -> Vec<u8> {
    b2_owner_chart_stream_with_encoding(carrier_class, true)
}

fn b2_owner_chart_stream_with_encoding(carrier_class: u8, width_coded: bool) -> Vec<u8> {
    let mut bytes = match carrier_class {
        0x28 => b2_cylinder_stream(),
        0x2b => b2_torus_stream(),
        0x32 => vec![0xa5, 0x03, 0x32, 0x00, 0x00, 0x00, 0x00, 0x05],
        _ => panic!("owner-chart fixture requires an admitted analytic carrier"),
    };
    let carrier_selector = match carrier_class {
        0x28 => 0x05,
        0x2b => 0x09,
        0x32 => 0x11,
        _ => unreachable!("carrier class checked above"),
    };
    let mut bridge = vec![
        0xb2, 0x03, 0x37, 0, 0x05, 0x85, 0x05, 0x04, 100, 0x03, 0x04, 101, 0x07,
    ];
    bridge.extend_from_slice(&[carrier_selector, 0x05]);
    bridge.extend_from_slice(&1.0f64.to_le_bytes());
    bridge.extend_from_slice(&[0x03, 0x05]);
    bridge.extend_from_slice(&[0; 8]);
    bridge.extend_from_slice(&[0x01, 0x05]);
    bridge[3] = u8::try_from(bridge.len() - 5).expect("owner-chart bridge length");
    bytes.extend_from_slice(&bridge);
    let (lower, upper, side_values) = match carrier_class {
        0x28 => (
            [2.0, 3.0],
            [5.0, 7.0],
            vec![
                vec![2.0, 3.0, 7.0],
                vec![5.0, 3.0, 7.0],
                vec![3.0, 2.0, 5.0],
                vec![7.0, 2.0, 5.0],
            ],
        ),
        0x2b => (
            [2.0, 3.0],
            [5.0, 7.0],
            vec![
                vec![3.0, 2.0, 5.0],
                vec![7.0, 2.0, 5.0],
                vec![2.0, 3.0, 7.0],
                vec![5.0, 3.0, 7.0],
            ],
        ),
        0x32 => (
            [0.0, 0.0],
            [596.25, 10.0],
            vec![
                vec![596.25],
                vec![10.0, 596.25],
                vec![10.0],
                vec![596.25, 10.0],
            ],
        ),
        _ => unreachable!("carrier class checked above"),
    };
    for (prefix, values) in [0x05, 0x09, 0x0d, 0x11].into_iter().zip(side_values) {
        let length = u8::try_from(2 + values.len() * size_of::<f64>())
            .expect("owner-chart parameter-point length");
        bytes.extend_from_slice(&[0xb2, 0x03, 0x18, length, 0x05, prefix, 0x12]);
        for value in values {
            bytes.extend_from_slice(&le_f64(value));
        }
    }
    let mut owner = vec![0xb2, 0x03, 0x62, 0, 0x05, 0x89];
    let references = if width_coded {
        [278, 1, 276, 2, 277, 3, 199, 4, 279]
    } else {
        [278, 324, 276, 268, 277, 374, 199, 195, 279]
    };
    for (index, value) in references.into_iter().enumerate() {
        if width_coded && index % 2 == 1 {
            owner.push(u8::try_from(value).expect("width-coded owner identity"));
        } else {
            owner.extend_from_slice(&compact_uint_bytes(value));
        }
    }
    owner.extend_from_slice(&owner_numeric_tail_for(lower, upper));
    owner[3] = u8::try_from(owner.len() - 5).expect("owner-chart packet length");
    bytes.extend_from_slice(&owner);
    bytes
}

pub(crate) fn b2_owner_chart_stream_with_extended_bridge() -> Vec<u8> {
    let mut bytes = b2_owner_chart_stream(0x32);
    let bridge_pos = bytes
        .windows(3)
        .position(|window| window == [0xb2, 0x03, 0x37])
        .expect("owner-chart bridge marker");
    let bridge_end = bridge_pos + 5 + usize::from(bytes[bridge_pos + 3]);
    let mut bridge = vec![
        0xb2, 0x03, 0x37, 0, 0x05, 0x88, 0x05, 0x04, 100, 0x03, 0x04, 101, 0x07, 0x0b, 0x0f, 0x13,
    ];
    bridge.extend_from_slice(&[0x11, 0x09, 0x05, 0x05]);
    bridge.extend_from_slice(&[0; 8]);
    bridge.extend_from_slice(&[0x01, 0x05]);
    bridge[3] = u8::try_from(bridge.len() - 5).expect("extended bridge length");
    bytes.splice(bridge_pos..bridge_end, bridge);
    bytes
}

pub(crate) fn b2_counted_61_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x61, 0x0c, 0x05, 0x84, 0x08, 0x14, 0x05, 0x08, 0x0e, 0x05, 0x79, 0x04, 0x4a,
        0x41, 0x03,
    ]
}

pub(crate) fn b2_long_61_stream() -> Vec<u8> {
    let mut payload = vec![0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb, 0x06];
    for member in [0x064a_u16, 0x0650, 0x0656] {
        payload.extend_from_slice(&member.to_le_bytes());
    }
    payload.push(0xfe);
    for reference in [0x0100_u16, 0x0103, 0x0106, 0x0109, 0x010c] {
        payload.push(0x0a);
        payload.extend_from_slice(&reference.to_le_bytes());
    }
    payload.extend_from_slice(&le_f64(42.5));
    payload.push(0x03);
    let mut record = vec![0xb2, 0x03, 0x61, u8::try_from(payload.len()).unwrap(), 0x05];
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn b2_class5b5c_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    let records = [
        (
            0xb2,
            0x13,
            0x5b,
            vec![0x81, 0x03, 0x05, 0x00, 0x08, 0x3a, 0x1c],
            vec![0x1f],
        ),
        (
            0xb3,
            0x03,
            0x5c,
            vec![0x81, 0x1f, 0x81, 0x01, 0x00, 0x01, 0x00, 0x05, 0x0d],
            vec![0x34, 0x12],
        ),
        (
            0xb4,
            0x83,
            0x5b,
            vec![0x42, 0x00, 0x7f],
            vec![0x01, 0x00, 0x10],
        ),
    ];
    for (lead, flag, class, payload, token) in records {
        assert_eq!(usize::from(lead - 0xb1), token.len());
        bytes.extend_from_slice(&[lead, flag, class, u8::try_from(payload.len()).unwrap()]);
        bytes.extend_from_slice(&token);
        bytes.extend_from_slice(&payload);
    }
    bytes
}

pub(crate) fn b2_face_node_5f_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x08, 0x5d, 0x02, 0x03, 0x05,
    ]
}

pub(crate) fn b2_adjacent_face_owner_stream() -> Vec<u8> {
    let mut bytes = vec![
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x08, 0xeb, 0x03, 0x03, 0x05,
    ];
    bytes.extend_from_slice(&b2_owner_packet_stream());
    bytes
}

pub(crate) fn b2_adjacent_secondary_face_owner_stream() -> Vec<u8> {
    let target = compact_uint_bytes(278);
    let mut bytes = vec![
        0xb2,
        0x03,
        0x5f,
        u8::try_from(1 + target.len() + 2).expect("face-node payload length"),
        0x05,
        0x82,
    ];
    bytes.extend_from_slice(&target);
    bytes.extend_from_slice(&[0x03, 0x03]);
    bytes.extend_from_slice(&b2_all_compact_owner_packet_stream());
    bytes
}

pub(crate) fn b2_adjacent_face_counted_owner_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x5f, 0x06, 0x11, 0x82, 0x08, 0x94, 0x03, 0x03, 0x05, 0xb2, 0x03, 0x62, 0x19,
        0x05, 0x87, 0x08, 0x8f, 0x03, 0x1d, 0x08, 0x07, 0x01, 0x08, 0x02, 0x01, 0x08, 0x19, 0x01,
        0x08, 0x14, 0x01, 0x08, 0x95, 0x03, 0x83, 0x41, 0x92, 0x00, 0x01,
    ]
}

pub(crate) fn b2_cone_face_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x3b, 0x20, 0x05];
    record.extend_from_slice(&[
        0x85, 0x05, 0x08, 0x7f, 0x05, 0x08, 0x14, 0x03, 0xe5, 0xdd, 0x05, 0x01, 0x01, 0x05, 0x03,
        0x11,
    ]);
    record.extend_from_slice(&le_f64(1.5));
    record.extend_from_slice(&le_f64(std::f64::consts::FRAC_PI_4));
    record
}

pub(crate) fn b2_cone_face_parameter_point_stream() -> Vec<u8> {
    let mut bytes = b2_cone_face_stream();
    bytes.extend_from_slice(&b2_parameter_point_stream());
    bytes
}

pub(crate) fn b2_topology_metadata_stream() -> Vec<u8> {
    let mut bytes = vec![
        0xb2, 0x03, 0x5e, 0x07, 0x05, 0x0a, 0x34, 0x12, 0x0a, 0x78, 0x56, 0,
    ];
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 1, 2, 3, 0x88]);
    bytes
}

pub(crate) fn b2_edge_node_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x5e, 0x0d, 0x05, 0x04, 0xd8, 0x08, 0x79, 0x03, 0x08, 0x7f, 0x03, 0x04, 0xd7,
        0x04, 0xd6, 0x21,
    ]
}

pub(crate) fn b2_line_profile_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x0e, 0x48, 0x05];
    for value in [1.0f64, 2.0, 3.0, 0.0, 0.6, 0.8, 1.0, -4.0, 9.0] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_revolution_stream() -> Vec<u8> {
    let scale = 2.0;
    let angular_lo = scale * 0.5;
    let angular_hi = angular_lo + scale * std::f64::consts::TAU;
    let mean = scale * (std::f64::consts::PI + 0.5);
    let mut record = vec![0xb2, 0x03, 0x2d, 0xae, 0x05];
    let mut payload = vec![0u8; 0xae];
    payload[0] = 0x0a;
    payload[1..3].copy_from_slice(&0x1234u16.to_le_bytes());
    let frame = [
        1.0f64, 2.0, 3.0, // origin
        1.0, 0.0, 0.0, // first basis
        0.0, 1.0, 0.0, // second basis
        0.0, 0.0, 1.0, // axis
    ];
    for (index, value) in frame.into_iter().enumerate() {
        payload[3 + 8 * index..11 + 8 * index].copy_from_slice(&le_f64(value));
    }
    for (index, value) in [angular_lo, angular_hi, -4.0, 9.0].into_iter().enumerate() {
        payload[99 + 8 * index..107 + 8 * index].copy_from_slice(&le_f64(value));
    }
    payload[131..133].copy_from_slice(&[0x05, 0x05]);
    payload[133..141].copy_from_slice(&le_f64(scale));
    payload[141..149].copy_from_slice(&le_f64(1.0));
    payload[149..157].copy_from_slice(&le_f64(1.0));
    payload[157..165].copy_from_slice(&le_f64(0.0));
    payload[165] = 0x01;
    payload[166..174].copy_from_slice(&le_f64(mean));
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn b2_resolved_revolution_stream() -> Vec<u8> {
    let mut circle = b2_circle_stream();
    circle[32..40].copy_from_slice(&le_f64(-4.0));
    circle[40..48].copy_from_slice(&le_f64(9.0));
    circle.extend_from_slice(&b2_revolution_stream());
    circle
}

pub(crate) fn b2_torus_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x2b, 200, 0x05];
    let mut values = [0.0f64; 25];
    values[0..3].copy_from_slice(&[1.0, 2.0, 3.0]);
    values[3..6].copy_from_slice(&[1.0, 0.0, 0.0]);
    values[6..9].copy_from_slice(&[0.0, 1.0, 0.0]);
    values[9..12].copy_from_slice(&[0.0, 0.0, 1.0]);
    values[12] = 7.0;
    values[13] = 2.0;
    values[14..18].copy_from_slice(&[
        std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
        0.0,
        std::f64::consts::TAU,
    ]);
    values[18..22].copy_from_slice(&[
        0.0,
        std::f64::consts::PI,
        -std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
    ]);
    values[22] = 14.0;
    values[23] = 4.0;
    for value in values {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_sphere_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x2a, 152, 0x05];
    let mut values = [0.0f64; 19];
    values[0..3].copy_from_slice(&[1.0, 2.0, 3.0]);
    values[3..6].copy_from_slice(&[5.0, 0.0, 0.0]);
    values[6..9].copy_from_slice(&[0.0, 5.0, 0.0]);
    values[9..12].copy_from_slice(&[0.0, 0.0, 5.0]);
    values[12] = 5.0;
    values[13..17].copy_from_slice(&[-2.0, 4.0, -1.0, std::f64::consts::FRAC_PI_2]);
    values[17] = values[12];
    values[18] = values[12] * ((values[13] + values[14]) * 0.5 - std::f64::consts::PI);
    for value in values {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_group_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x65, 0x04, 0x05, 0x81, 0x03, 0x05, 0x0d, 0xb2, 0x03, 0x60, 0x02, 0x05, 0x81,
        0x0d,
    ]
}

pub(crate) fn b2_offset_support_stream() -> Vec<u8> {
    b2_offset_support_stream_for([0.0, -1.0, 4.0, 3.0])
}

pub(crate) fn b2_offset_support_stream_for(domain: [f64; 4]) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x31, 0x2b, 0x05, 0x08, 0x34, 0x12];
    for value in [2.5f64, domain[0], domain[1], domain[2], domain[3]] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b3_offset_support_stream() -> Vec<u8> {
    let narrow = b2_offset_support_stream();
    let mut wide = vec![0xb3, 0x03, 0x31, narrow[3], 0x05, 0x00];
    wide.extend_from_slice(&narrow[5..]);
    wide
}

pub(crate) fn b2_edge_parameter_stream() -> Vec<u8> {
    b2_edge_parameter_stream_for(2.0, 7.0)
}

pub(crate) fn b2_edge_parameter_stream_for(lo: f64, hi: f64) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x23, 0x4e, 0];
    record.extend_from_slice(&[0; 6]);
    for value in [lo, hi, 1e-6, lo, hi, 1.0, lo, hi, 1e-6] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_edge_block_stream() -> Vec<u8> {
    fn b_family_pcurve() -> Vec<u8> {
        let a_family = a5_pcurve_stream();
        let payload = &a_family[8..];
        let mut record = vec![
            0xb2,
            0x03,
            0x20,
            u8::try_from(payload.len()).unwrap(),
            a_family[7],
        ];
        record.extend_from_slice(payload);
        record
    }

    let mut bytes = b_family_pcurve();
    bytes.extend_from_slice(&b_family_pcurve());
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes
}

pub(crate) fn b2_topology_edge_run_stream() -> Vec<u8> {
    let mut bytes = b2_edge_block_stream();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 5, 9, 0x84]);
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 9, 13, 0x88]);
    bytes.extend_from_slice(&b2_edge_node_stream());
    bytes
}

pub(crate) fn b2_circle_stream() -> Vec<u8> {
    let radius = 3.0;
    let mut record = vec![0xb2, 0x03, 0x19, 0x34, 0x05, 0x08, 0x34, 0x12];
    for value in [
        4.0f64,
        -2.0,
        radius,
        0.0,
        2.0 * std::f64::consts::PI * radius,
    ] {
        record.extend_from_slice(&le_f64(value));
    }
    record.push(0x01);
    record.extend_from_slice(&le_f64(0.0));
    record
}

pub(crate) fn b2_cylinder_stream() -> Vec<u8> {
    let radius = 2.0;
    let mut record = vec![0xb2, 0x03, 0x28, 0x5a, 0x05];
    record.resize(95, 0);
    let p = 5;
    for (index, value) in [1.0f64, 2.0, 3.0].into_iter().enumerate() {
        record[p + 8 * index..p + 8 * index + 8].copy_from_slice(&le_f64(value));
    }
    record[p + 24] = 0x19;
    record[p + 25..p + 33].copy_from_slice(&le_f64(1.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(0.0));
    record[p + 41..p + 49].copy_from_slice(&le_f64(1.0));
    record[p + 49..p + 57].copy_from_slice(&le_f64(radius));
    record[p + 57..p + 65].copy_from_slice(&le_f64(0.0));
    record[p + 65..p + 73].copy_from_slice(&le_f64(2.0 * std::f64::consts::PI * radius));
    record[p + 73..p + 81].copy_from_slice(&le_f64(-4.0));
    record[p + 81..p + 89].copy_from_slice(&le_f64(5.0));
    record[p + 89] = 0x07;
    record
}

pub(crate) fn b3_cylinder_stream() -> Vec<u8> {
    let narrow = b2_cylinder_stream();
    let mut wide = vec![0xb3, 0x03, 0x28, 0x5a, 0x05, 0x00];
    wide.extend_from_slice(&narrow[5..]);
    wide
}

pub(crate) fn b2_implicit_axis_cylinder_stream() -> Vec<u8> {
    let radius = 2.0;
    let mut record = vec![0xb2, 0x03, 0x28, 0x52, 0x05];
    record.resize(87, 0);
    let p = 5;
    record[p + 24] = 0x1d;
    record[p + 25..p + 33].copy_from_slice(&le_f64(1.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(1.0));
    record[p + 41..p + 49].copy_from_slice(&le_f64(radius));
    record[p + 49..p + 57].copy_from_slice(&le_f64(0.0));
    record[p + 57..p + 65].copy_from_slice(&le_f64(2.0 * std::f64::consts::PI * radius));
    record[p + 65..p + 73].copy_from_slice(&le_f64(-1.0));
    record[p + 73..p + 81].copy_from_slice(&le_f64(3.0));
    record[p + 81] = 0x07;
    record
}

pub(crate) fn b2_range_origin_cylinder_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x28, 0x62, 0x05];
    record.resize(103, 0);
    let p = 5;
    record[p + 24] = 0x0e;
    record[p + 25..p + 33].copy_from_slice(&le_f64(0.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(1.0));
    record[p + 41..p + 49].copy_from_slice(&le_f64(1.0));
    record[p + 49..p + 57].copy_from_slice(&le_f64(4.0));
    record[p + 57..p + 65].copy_from_slice(&le_f64(0.0));
    record[p + 65..p + 73].copy_from_slice(&le_f64(8.0));
    record[p + 73..p + 81].copy_from_slice(&le_f64(-2.0));
    record[p + 81..p + 89].copy_from_slice(&le_f64(2.0));
    record[p + 89] = 0x03;
    let range_origin = (0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0;
    record[p + 90..p + 98].copy_from_slice(&le_f64(range_origin));
    record
}

pub(crate) fn b2_cone_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x29, 0xb8, 0x05];
    record.resize(189, 0);
    for (start, values) in [
        (5, [1.0f64, 2.0, 3.0]),
        (29, [1.0, 0.0, 0.0]),
        (53, [0.0, 1.0, 0.0]),
        (77, [0.0, 0.0, 1.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            record[start + 8 * index..start + 8 * index + 8].copy_from_slice(&le_f64(value));
        }
    }
    record[101..109].copy_from_slice(&le_f64(0.25));
    record[109..117].copy_from_slice(&le_f64(4.0));
    record[117..125].copy_from_slice(&le_f64(0.5));
    record[125..133].copy_from_slice(&le_f64(0.5 + std::f64::consts::PI));
    record[133..141].copy_from_slice(&le_f64(2.0));
    record[141..149].copy_from_slice(&le_f64(8.0));
    record[149..157].copy_from_slice(&le_f64(3.0));
    record[157..165].copy_from_slice(&le_f64(1.0));
    record[173..181].copy_from_slice(&le_f64(0.5 - std::f64::consts::FRAC_PI_2));
    record[181..189].copy_from_slice(&le_f64(0.5 + 3.0 * std::f64::consts::FRAC_PI_2));
    record
}

pub(crate) fn b2_construction_use_stream() -> Vec<u8> {
    b2_construction_use_stream_for([0.0, -1.0, 4.0, 3.0])
}

pub(crate) fn b2_construction_use_stream_for(domain: [f64; 4]) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x30, 0x2d, 0x05, 0x05, 0x08, 0x34, 0x12];
    record.extend_from_slice(&le_f64(-2.0));
    record.push(0x01);
    for value in [domain[0], domain[2], domain[1], domain[3]] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_embedded_cylinder_stream() -> Vec<u8> {
    b2_embedded_cylinder_stream_with_object_id(0x5678)
}

pub(crate) fn b2_embedded_cylinder_stream_with_object_id(object_id: u32) -> Vec<u8> {
    let standalone = b2_cylinder_stream();
    let mut record = vec![
        0xb2, 0x03, 0x60, 0x02, 0x05, 0x81, 0x0d, 0xb4, 0x03, 0x28, 0x5a,
    ];
    record.extend_from_slice(&compact_uint_bytes(object_id));
    record.extend_from_slice(&standalone[5..]);
    record
}

pub(crate) fn inner_no_directory_b2_catpart() -> Vec<u8> {
    let mut file = object_main_catpart(&b2_cylinder_stream());
    let name = b"M\x00a\x00i\x00n\x00D\x00a\x00t\x00a\x00S\x00t\x00r\x00e\x00a\x00m\x00";
    let pos = file
        .windows(name.len())
        .position(|bytes| bytes == name)
        .expect("main stream name");
    file[pos] = b'X';
    file
}
