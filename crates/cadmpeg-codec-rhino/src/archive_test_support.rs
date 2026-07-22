// SPDX-License-Identifier: Apache-2.0

use crate::chunks::TCODE_ENDOFFILE;
use crate::MAGIC;

pub(crate) const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const LINE_CLASS: [u8; 16] = [
    0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const SUBD_CLASS: [u8; 16] = [
    0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85, 0x3b,
];
pub(crate) const ARC_CLASS: [u8; 16] = [
    0x2a, 0xbe, 0x33, 0xcf, 0xb4, 0x09, 0xd4, 0x11, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const POLYLINE_CLASS: [u8; 16] = [
    0xe6, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const POLYCURVE_CLASS: [u8; 16] = [
    0xe0, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const POINT_CLOUD_CLASS: [u8; 16] = [
    0x47, 0xf3, 0x88, 0x24, 0xfa, 0xf8, 0xd3, 0x11, 0xbf, 0xec, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
pub(crate) const EXTRUSION_CLASS: [u8; 16] = [
    0x75, 0x31, 0xf5, 0x36, 0xb8, 0x72, 0x47, 0x4d, 0xbf, 0x1f, 0xb4, 0xe6, 0xfc, 0x24, 0xf4, 0xb9,
];
pub(crate) const BREP_CLASS: [u8; 16] = [
    0xc5, 0xdb, 0xb5, 0x60, 0x60, 0xe6, 0xd3, 0x11, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const PLANE_SURFACE_CLASS: [u8; 16] = [
    0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

pub(crate) fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend((body.len() as i64).to_le_bytes());
    bytes.extend(body);
    bytes
}

pub(crate) fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(typecode, &payload)
}

pub(crate) fn short_chunk(typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | 0x8000_0000).to_le_bytes().to_vec();
    bytes.extend(value.to_le_bytes());
    bytes
}

pub(crate) fn table(typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(0x7fff_ffff, 0));
    long_chunk(typecode, &body)
}

pub(crate) fn object_record(object_type: i64, class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let object_type = short_chunk(0x0200_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(0x0002_fffc, payload);
    let class_end = short_chunk(0x0002_7fff, 0);
    let class = long_chunk(0x0002_7ffa, &[uuid, class_data, class_end].concat());
    let object_end = short_chunk(0x0200_007f, 0);
    crc_chunk(
        0x2000_8070 | 0x0000_8000,
        &[object_type, class, object_end].concat(),
    )
}

pub(crate) fn class_wrapper(class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(0x0002_fffc, payload);
    let class_end = short_chunk(0x0002_7fff, 0);
    long_chunk(0x0002_7ffa, &[uuid, class_data, class_end].concat())
}

pub(crate) fn point_payload(point: [f64; 3]) -> Vec<u8> {
    let mut payload = vec![0x10];
    for coordinate in point {
        payload.extend(coordinate.to_le_bytes());
    }
    payload
}

pub(crate) fn line_payload(origin: [f64; 3], direction: [f64; 3], interval: [f64; 2]) -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in origin.into_iter().chain(direction).chain(interval) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    payload
}

pub(crate) fn point_cloud_payload(points: &[[f64; 3]]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend((points.len() as i32).to_le_bytes());
    payload.extend(
        points
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes()),
    );
    payload.extend(
        [
            0.0_f64, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x
            0.0, 1.0, 0.0, // y
            0.0, 0.0, 1.0, // z
            0.0, 0.0, 1.0, 0.0, // equation
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes),
    );
    payload.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
    payload.extend(0_i32.to_le_bytes());
    payload
}

pub(crate) fn arc_payload(angle: [f64; 2], domain: [f64; 2]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(
        [
            0.0_f64, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x
            0.0, 1.0, 0.0, // y
            0.0, 0.0, 1.0, // z
            0.0, 0.0, 1.0, 0.0, // equation
            2.0, // radius
            2.0, 0.0, 0.0, // angle zero
            0.0, 2.0, 0.0, // half pi
            -2.0, 0.0, 0.0, // pi
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes),
    );
    payload.extend(angle.into_iter().flat_map(f64::to_le_bytes));
    payload.extend(domain.into_iter().flat_map(f64::to_le_bytes));
    payload.extend(3_i32.to_le_bytes());
    payload
}

pub(crate) fn polyline_payload(points: &[[f64; 3]], parameters: &[f64]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend((points.len() as i32).to_le_bytes());
    payload.extend(
        points
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes()),
    );
    payload.extend((parameters.len() as i32).to_le_bytes());
    payload.extend(parameters.iter().flat_map(|value| value.to_le_bytes()));
    payload.extend(3_i32.to_le_bytes());
    payload
}

pub(crate) fn polycurve_payload(parameters: &[f64], children: &[([u8; 16], Vec<u8>)]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend((children.len() as i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 48]);
    payload.extend((parameters.len() as i32).to_le_bytes());
    payload.extend(parameters.iter().flat_map(|value| value.to_le_bytes()));
    for (uuid, child) in children {
        payload.extend(class_wrapper(*uuid, child));
    }
    payload
}

fn mesh_buffer(bytes: &[u8]) -> Vec<u8> {
    let mut result = (bytes.len() as u32).to_le_bytes().to_vec();
    if !bytes.is_empty() {
        result.extend(crc32fast::hash(bytes).to_le_bytes());
        result.push(0);
        result.extend(bytes);
    }
    result
}

fn mesh_common(version: u8) -> Vec<u8> {
    let mut payload = vec![version];
    payload.extend(4_i32.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    for _ in 0..4 {
        payload.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
    }
    payload.extend([0.0_f64; 2].into_iter().flat_map(f64::to_le_bytes));
    payload.extend([0.0_f32; 16].into_iter().flat_map(f32::to_le_bytes));
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 5]);
    payload.extend(1_i32.to_le_bytes());
    payload.extend([0, 1, 2, 2, 0, 2, 3, 1]);
    payload
}

pub(crate) fn mesh_payload(major: u8, minor: u8, bad_vertex_crc: bool, mapping: bool) -> Vec<u8> {
    let mut payload = mesh_common((major << 4) | minor);
    let vertices = [
        [0.0_f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let vertex_bytes = vertices
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let normals = [[0.0_f32, 0.0, 1.0]; 4]
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let uv = [[0.0_f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let colors = [[255_u8, 0, 0, 255]; 4].concat();
    if major == 1 {
        for (count, bytes) in [
            (4_i32, vertex_bytes),
            (4, normals),
            (4, uv),
            (0, Vec::new()),
            (4, colors),
        ] {
            payload.extend(count.to_le_bytes());
            payload.extend(bytes);
        }
    } else {
        for bytes in [vertex_bytes, normals, uv, Vec::new(), colors] {
            let start = payload.len();
            payload.extend(mesh_buffer(&bytes));
            if bad_vertex_crc && start == mesh_common((major << 4) | minor).len() {
                payload[start + 4..start + 8].copy_from_slice(&0_u32.to_le_bytes());
            }
        }
    }
    if minor >= 2 {
        payload.extend(0_i32.to_le_bytes());
    }
    if major == 3 && minor >= 3 {
        payload.extend([0_u8; 16]);
        let parameters = [[0.0_f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        payload.extend(mesh_buffer(&parameters));
    }
    if major == 3 && minor >= 4 && mapping {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(1_i32.to_le_bytes());
        body.extend([0_u8; 16]);
        body.extend(7_i32.to_le_bytes());
        for index in 0..16 {
            body.extend(f64::from((index % 5 == 0) as u8).to_le_bytes());
        }
        body.extend(3_u32.to_le_bytes());
        payload.extend(crc_chunk(0x4000_8000, &body));
    }
    if major == 3 && minor >= 5 {
        payload.extend([0_u8; 3]);
    }
    if major == 3 && minor >= 6 {
        payload.push(1);
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(1_u32.to_le_bytes());
        body.extend(4_u32.to_le_bytes());
        body.extend(2_u32.to_le_bytes());
        body.extend([0_u32, 1, 2, 3].into_iter().flat_map(u32::to_le_bytes));
        body.extend([0_u32, 1].into_iter().flat_map(u32::to_le_bytes));
        payload.extend(crc_chunk(0x4000_8000, &body));
    }
    if major == 3 && minor >= 7 {
        payload.push(1);
        let doubles = vertices
            .iter()
            .flatten()
            .flat_map(|value| f64::from(*value).to_le_bytes())
            .collect::<Vec<_>>();
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(4_u32.to_le_bytes());
        body.extend(mesh_buffer(&doubles));
        payload.extend(crc_chunk(0x4000_8000, &body));
    }
    if major == 3 && minor >= 8 {
        payload.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
    }
    payload
}

fn packed_array(records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![0x10];
    body.extend((records.len() as i32).to_le_bytes());
    body.extend(records.concat());
    crc_chunk(0x4000_8000, &body)
}

fn indexes(values: &[i32]) -> Vec<u8> {
    let mut bytes = (values.len() as i32).to_le_bytes().to_vec();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    bytes
}

fn plane_surface_payload() -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(
        [
            0.0_f64, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x
            0.0, 1.0, 0.0, // y
            0.0, 0.0, 1.0, // z
            0.0, 0.0, 1.0, 0.0, // equation
            0.0, 1.0, // u
            0.0, 1.0, // v
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes),
    );
    payload
}

fn brep_children(class: [u8; 16], payload: Vec<u8>) -> Vec<u8> {
    brep_children_many(&[(class, payload)])
}

fn brep_children_many(children: &[([u8; 16], Vec<u8>)]) -> Vec<u8> {
    let mut body = vec![0x10];
    body.extend((children.len() as i32).to_le_bytes());
    let mut direct = body.clone();
    for (class, payload) in children {
        body.extend(1_i32.to_le_bytes());
        direct.extend(1_i32.to_le_bytes());
        body.extend(class_wrapper(*class, payload));
    }
    let mut payload = body;
    payload.extend(crc32fast::hash(&direct).to_le_bytes());
    long_chunk(0x4000_8000, &payload)
}

fn region_array(records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend((records.len() as i32).to_le_bytes());
    for record in records {
        let mut element = 1_i32.to_le_bytes().to_vec();
        element.extend(0_i32.to_le_bytes());
        element.extend(record);
        body.extend(crc_chunk(0x4000_8000, &element));
    }
    crc_chunk(0x4000_8000, &body)
}

pub(crate) fn brep_payload(semantic_invalid: bool) -> Vec<u8> {
    brep_payload_with_topology(false, semantic_invalid)
}

pub(crate) fn singular_seam_brep_payload(malformed_ring: bool) -> Vec<u8> {
    brep_payload_with_topology(true, malformed_ring)
}

fn brep_payload_with_topology(singular_seam: bool, malformed: bool) -> Vec<u8> {
    let mut payload = vec![0x33];
    // The triangle's trim curves in plane parameter space, one per side; the
    // seam fixture reuses one side four times.
    let triangle = [
        ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
    ];
    let c2_lines: Vec<([u8; 16], Vec<u8>)> = if singular_seam {
        // Trim order: singular at u=0, seam side, singular at u=1, seam side.
        // Singular trims hug their vertex's parameter point; the C2 reader
        // rejects zero-length lines, so they span less than the coincidence
        // tolerance instead of collapsing exactly.
        [
            ([0.0, 0.0, 0.0], [0.005, 0.0, 0.0]),
            ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.995, 0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        ]
        .iter()
        .map(|(from, to)| {
            let mut c2 = line_payload(*from, *to, [0.0, 1.0]);
            let end = c2.len();
            c2[end - 4..].copy_from_slice(&2_i32.to_le_bytes());
            (LINE_CLASS, c2)
        })
        .collect()
    } else {
        triangle
            .iter()
            .map(|(from, to)| {
                let mut c2 = line_payload(*from, *to, [0.0, 1.0]);
                let end = c2.len();
                c2[end - 4..].copy_from_slice(&2_i32.to_le_bytes());
                (LINE_CLASS, c2)
            })
            .collect()
    };
    payload.extend(brep_children_many(&c2_lines));
    let c3_lines: Vec<([u8; 16], Vec<u8>)> = if singular_seam {
        vec![(
            LINE_CLASS,
            line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        )]
    } else {
        triangle
            .iter()
            .map(|(from, to)| (LINE_CLASS, line_payload(*from, *to, [0.0, 1.0])))
            .collect()
    };
    payload.extend(brep_children_many(&c3_lines));
    payload.extend(brep_children(PLANE_SURFACE_CLASS, plane_surface_payload()));

    let vertices = if singular_seam {
        vec![
            ([0.0_f64, 0.0, 0.0], vec![0_i32]),
            ([1.0, 0.0, 0.0], vec![0]),
        ]
    } else {
        vec![
            ([0.0_f64, 0.0, 0.0], vec![0_i32, 2_i32]),
            ([1.0, 0.0, 0.0], vec![0, 1]),
            ([0.0, 1.0, 0.0], vec![1, 2]),
        ]
    }
    .into_iter()
    .enumerate()
    .map(|(index, (point, edges))| {
        let mut record = (index as i32).to_le_bytes().to_vec();
        record.extend(point.into_iter().flat_map(f64::to_le_bytes));
        record.extend(indexes(&edges));
        record.extend(0.02_f64.to_le_bytes());
        record
    })
    .collect::<Vec<_>>();
    payload.extend(packed_array(&vertices));

    let edge_records = if singular_seam {
        vec![([0_i32, 1_i32], vec![1_i32, 3_i32])]
    } else {
        vec![
            ([0_i32, 1_i32], vec![0_i32]),
            ([1, 2], vec![1]),
            ([2, 0], vec![2]),
        ]
    };
    let edges = edge_records
        .iter()
        .enumerate()
        .map(|(index, (vertices, trims))| {
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend(
                (if malformed && !singular_seam && index == 0 {
                    7_i32
                } else if singular_seam {
                    0
                } else {
                    index as i32
                })
                .to_le_bytes(),
            );
            record.extend(0_i32.to_le_bytes());
            record.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
            record.extend(vertices.iter().flat_map(|value| value.to_le_bytes()));
            record.extend(indexes(trims));
            record.extend(0.03_f64.to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(packed_array(&edges));

    let trim_records = if singular_seam {
        vec![
            (0_i32, -1_i32, [0_i32, 0_i32], 0_i32, 4_i32, 4_i32),
            (1, 0, [0, 1], 0, 3, 5),
            (2, -1, [1, 1], 0, 4, 6),
            (3, 0, if malformed { [0, 1] } else { [1, 0] }, 1, 3, 3),
        ]
    } else {
        vec![
            (0_i32, 0_i32, [0_i32, 1_i32], 0_i32, 1_i32, 0_i32),
            (1, 1, [1, 2], 0, 1, 0),
            (2, 2, [2, 0], 0, 1, 0),
        ]
    };
    let trims = trim_records
        .iter()
        .enumerate()
        .map(
            |(index, (curve, edge, vertices, reversed_3d, trim_type, iso))| {
                let mut record = (index as i32).to_le_bytes().to_vec();
                record.extend(curve.to_le_bytes());
                record.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
                record.extend(edge.to_le_bytes());
                record.extend(vertices.iter().flat_map(|value| value.to_le_bytes()));
                record.extend(reversed_3d.to_le_bytes());
                record.extend(trim_type.to_le_bytes());
                record.extend(iso.to_le_bytes());
                record.extend(0_i32.to_le_bytes());
                record.extend([0.04_f64, 0.05].into_iter().flat_map(f64::to_le_bytes));
                record.extend([0_u8; 48]);
                record.extend([0.04_f64, 0.05].into_iter().flat_map(f64::to_le_bytes));
                record
            },
        )
        .collect::<Vec<_>>();
    payload.extend(packed_array(&trims));

    let mut loop_record = 0_i32.to_le_bytes().to_vec();
    loop_record.extend(indexes(
        &(0..i32::try_from(trim_records.len()).expect("small fixture")).collect::<Vec<_>>(),
    ));
    loop_record.extend(1_i32.to_le_bytes());
    loop_record.extend(0_i32.to_le_bytes());
    payload.extend(packed_array(&[loop_record]));

    let mut face_body = vec![0x10];
    face_body.extend(1_i32.to_le_bytes());
    face_body.extend(0_i32.to_le_bytes());
    face_body.extend(indexes(&[0]));
    face_body.extend(0_i32.to_le_bytes());
    face_body.extend(0_i32.to_le_bytes());
    face_body.extend(0_i32.to_le_bytes());
    payload.extend(crc_chunk(0x4000_8000, &face_body));
    payload.extend(
        [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    payload.extend(crc_chunk(0x4000_8000, &[0, 0]));
    payload.extend(crc_chunk(0x4000_8000, &[0, 0]));
    payload.extend(0_i32.to_le_bytes());

    let sides = [[0_i32, 1, 0, 1], [1_i32, 0, 0, -1]]
        .into_iter()
        .map(|values| {
            values
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let regions = [(0_i32, 0_i32, 1_i32), (1, 1, 0)]
        .into_iter()
        .map(|(index, kind, side)| {
            let mut record = index.to_le_bytes().to_vec();
            record.extend(kind.to_le_bytes());
            record.extend(indexes(&[side]));
            record.extend(
                [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0]
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record
        })
        .collect::<Vec<_>>();
    let mut topology = 1_i32.to_le_bytes().to_vec();
    topology.extend(0_i32.to_le_bytes());
    topology.extend(region_array(&sides));
    topology.extend(region_array(&regions));
    let mut outer = 1_i32.to_le_bytes().to_vec();
    outer.extend(0_i32.to_le_bytes());
    outer.push(1);
    outer.extend(crc_chunk(0x4000_8000, &topology));
    payload.extend(crc_chunk(0x4000_8000, &outer));
    payload
}

fn units_record(unit: i32) -> Vec<u8> {
    let mut body = 100_i32.to_le_bytes().to_vec();
    body.extend(unit.to_le_bytes());
    body.extend(0.01_f64.to_le_bytes());
    body.extend(0.1_f64.to_le_bytes());
    body.extend(0.001_f64.to_le_bytes());
    crc_chunk(0x2000_8031, &body)
}

pub(crate) fn archive(objects: &[Vec<u8>]) -> Vec<u8> {
    archive_version("50", objects)
}

pub(crate) fn archive_version(version: &str, objects: &[Vec<u8>]) -> Vec<u8> {
    archive_version_unit(version, 2, None, objects)
}

pub(crate) fn archive_unit(unit: i32, objects: &[Vec<u8>]) -> Vec<u8> {
    archive_version_unit("50", unit, None, objects)
}

pub(crate) fn archive_writer(version: &str, writer_version: i64, objects: &[Vec<u8>]) -> Vec<u8> {
    archive_version_unit(version, 2, Some(writer_version), objects)
}

fn archive_version_unit(
    version: &str,
    unit: i32,
    writer_version: Option<i64>,
    objects: &[Vec<u8>],
) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    let mut version_field = [b' '; 8];
    let start = version_field.len() - version.len();
    version_field[start..].copy_from_slice(version.as_bytes());
    bytes.extend(version_field);
    bytes.extend(long_chunk(1, b"cadmpeg synthetic archive"));
    let properties = writer_version
        .map(|value| vec![short_chunk(0x2000_0026, value)])
        .unwrap_or_default();
    bytes.extend(table(0x1000_0014, &properties));
    bytes.extend(table(0x1000_0015, &[units_record(unit)]));
    bytes.extend(table(0x1000_0013, objects));
    let eof_offset = bytes.len();
    bytes.extend(long_chunk(TCODE_ENDOFFILE, &[0; 8]));
    let eof = long_chunk(TCODE_ENDOFFILE, &(bytes.len() as u64).to_le_bytes());
    bytes[eof_offset..].copy_from_slice(&eof);
    bytes
}
