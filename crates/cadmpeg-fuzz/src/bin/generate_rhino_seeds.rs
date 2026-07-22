// SPDX-License-Identifier: Apache-2.0
//! Generates deterministic synthetic seeds for the focused Rhino fuzz targets.

use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::write::ZlibEncoder;
use flate2::Compression;

const MAGIC: &[u8] = b"3D Geometry File Format ";

fn main() {
    generate_container_seeds();
    generate_chunk_seeds();
    generate_object_seeds();
    generate_nurbs_seeds();
    generate_mesh_seeds();
    generate_brep_seeds();
    generate_subd_seeds();
}

fn replace(directory: &str, seeds: &[(&str, Vec<u8>)]) {
    let path = Path::new(directory);
    fs::create_dir_all(path).expect("required invariant");
    for entry in fs::read_dir(path).expect("required invariant") {
        let entry = entry.expect("required invariant").path();
        if entry.is_dir() {
            fs::remove_dir_all(entry).expect("required invariant");
        } else {
            fs::remove_file(entry).expect("required invariant");
        }
    }
    for (name, bytes) in seeds {
        fs::write(path.join(name), bytes).expect("required invariant");
    }
}

fn write_seed(directory: &str, name: &str, bytes: &[u8]) {
    let path = Path::new(directory);
    fs::create_dir_all(path).expect("required invariant");
    fs::write(path.join(name), bytes).expect("required invariant");
}

fn header(version: u64) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(format!("{version:>8}").as_bytes());
    bytes
}

fn value_width(version: u64) -> usize {
    if version >= 50 {
        8
    } else {
        4
    }
}

fn long_chunk(version: u64, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if value_width(version) == 8 {
        bytes.extend((body.len() as i64).to_le_bytes());
    } else {
        bytes.extend((body.len() as i32).to_le_bytes());
    }
    bytes.extend(body);
    bytes
}

fn short_chunk(version: u64, typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | 0x8000_0000).to_le_bytes().to_vec();
    if value_width(version) == 8 {
        bytes.extend(value.to_le_bytes());
    } else {
        bytes.extend((value as i32).to_le_bytes());
    }
    bytes
}

fn crc_chunk(version: u64, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut with_crc = body.to_vec();
    with_crc.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(version, typecode | 0x8000, &with_crc)
}

fn minimal_document(version: u64) -> Vec<u8> {
    let mut bytes = header(version);
    bytes.extend(long_chunk(version, 1, b"fuzz"));
    let eof_offset = bytes.len();
    let eof_body_width = value_width(version);
    let eof_len = 4 + value_width(version) + eof_body_width;
    let final_size = eof_offset + eof_len;
    let body = if eof_body_width == 8 {
        (final_size as u64).to_le_bytes().to_vec()
    } else {
        (final_size as u32).to_le_bytes().to_vec()
    };
    bytes.extend(long_chunk(version, 0x7fff, &body));
    bytes
}

fn generate_container_seeds() {
    let versions = [1_u64, 2, 3, 4, 5, 50, 60, 70, 80];
    let mut seeds = versions
        .into_iter()
        .map(|version| (format!("archive_{version}"), minimal_document(version)))
        .collect::<Vec<_>>();
    let valid = minimal_document(50);
    seeds.push(("truncated".into(), valid[..valid.len() / 2].to_vec()));
    let mut oversize = header(50);
    oversize.extend(1_u32.to_le_bytes());
    oversize.extend(i64::MAX.to_le_bytes());
    seeds.push(("oversize".into(), oversize));
    let borrowed = seeds
        .iter()
        .map(|(name, data)| (name.as_str(), data.clone()))
        .collect::<Vec<_>>();
    replace("seeds/rhino_container", &borrowed);

    let mut mutated = vec![0_u8];
    mutated.extend(minimal_document(80));
    write_seed("seeds/decode_pipeline_mutated", "rhino_minimal", &mutated);
}

fn generate_chunk_seeds() {
    let short = short_chunk(50, 0x1234, 7);
    let long = long_chunk(50, 0x1234, b"body");
    let crc = crc_chunk(50, 0x1234, b"body");
    let mut mismatch = crc.clone();
    *mismatch.last_mut().expect("required invariant") ^= 0xff;
    replace(
        "seeds/rhino_chunks",
        &[
            ("short", short),
            ("long", long),
            ("crc_valid", crc),
            ("crc_mismatch", mismatch),
            ("truncated", vec![0x34, 0x12, 0, 0]),
        ],
    );
}

fn object_body(version: u64, payload: &[u8]) -> Vec<u8> {
    let object_type = short_chunk(version, 0x02a0_0071, 1);
    let class_uuid = [
        0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let uuid = crc_chunk(version, 0x0002_7ffb, &class_uuid);
    let data = crc_chunk(version, 0x0002_7ffc, payload);
    let class_end = short_chunk(version, 0x0202_7fff, 0);
    let class = long_chunk(version, 0x0002_7ffa, &[uuid, data, class_end].concat());
    let object_end = short_chunk(version, 0x02a0_007f, 0);
    [object_type, class, object_end].concat()
}

fn generate_object_seeds() {
    let mut valid = vec![5];
    valid.extend(object_body(50, &nurbs_curve(false, true)));
    let mut truncated = valid.clone();
    truncated.truncate(truncated.len() - 5);
    let mut mismatch = valid.clone();
    if let Some(byte) = mismatch.get_mut(24) {
        *byte ^= 0x80;
    }
    replace(
        "seeds/rhino_object_record",
        &[
            ("one_object", valid),
            ("truncated", truncated),
            ("crc_mismatch", mismatch),
            (
                "oversize",
                [
                    vec![5],
                    0x82a0_0071_u32.to_le_bytes().to_vec(),
                    i64::MAX.to_le_bytes().to_vec(),
                ]
                .concat(),
            ),
        ],
    );
}

fn push_f64s(bytes: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}

fn nurbs_curve(rational: bool, clamped: bool) -> Vec<u8> {
    let mut bytes = vec![0x10];
    for value in [3_i32, i32::from(rational), 2, 2, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0; 48]);
    bytes.extend(2_i32.to_le_bytes());
    push_f64s(
        &mut bytes,
        if clamped { &[0.0, 1.0] } else { &[0.25, 0.75] },
    );
    bytes.extend(2_i32.to_le_bytes());
    if rational {
        push_f64s(&mut bytes, &[0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 2.0]);
    } else {
        push_f64s(&mut bytes, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
    bytes
}

fn nurbs_surface() -> Vec<u8> {
    let mut bytes = vec![0x10];
    for value in [3_i32, 0, 2, 2, 2, 2, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0; 48]);
    for _ in 0..2 {
        bytes.extend(2_i32.to_le_bytes());
        push_f64s(&mut bytes, &[0.0, 1.0]);
    }
    bytes.extend(4_i32.to_le_bytes());
    push_f64s(
        &mut bytes,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
    );
    bytes
}

fn plane_surface() -> Vec<u8> {
    let mut bytes = vec![0x10];
    push_f64s(
        &mut bytes,
        &[
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
            1.0, 0.0, 1.0,
        ],
    );
    bytes
}

fn generate_nurbs_seeds() {
    let mut clamped = vec![0, 5];
    clamped.extend(nurbs_curve(false, true));
    let truncated = clamped[..clamped.len() / 2].to_vec();
    let mut nonclamped = vec![0, 5];
    nonclamped.extend(nurbs_curve(false, false));
    let mut rational = vec![0, 8];
    rational.extend(nurbs_curve(true, true));
    let mut surface = vec![1, 6];
    surface.extend(nurbs_surface());
    let mut plane = vec![2, 7];
    plane.extend(plane_surface());
    let mut oversize = vec![0, 5, 0x10];
    oversize.extend(
        [3_i32, 0, 2, i32::MAX, 0, 0]
            .into_iter()
            .flat_map(i32::to_le_bytes),
    );
    replace(
        "seeds/rhino_nurbs",
        &[
            ("curve_clamped", clamped),
            ("curve_nonclamped", nonclamped),
            ("curve_rational", rational),
            ("surface", surface),
            ("plane", plane),
            ("truncated_curve", truncated),
            ("oversize", oversize),
        ],
    );
}

fn mesh_buffer(raw: &[u8], method: u8) -> Vec<u8> {
    let body = if method == 0 {
        raw.to_vec()
    } else {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(raw).expect("required invariant");
        encoder.finish().expect("required invariant")
    };
    let mut bytes = (raw.len() as u16).to_le_bytes().to_vec();
    bytes.extend((raw.len() as u32).to_le_bytes());
    bytes.extend(crc32fast::hash(raw).to_le_bytes());
    bytes.push(method);
    bytes.extend(body);
    bytes
}

fn generate_mesh_seeds() {
    let raw = [0_u8, 0, 128, 63, 0, 0, 0, 64];
    let zlib = mesh_buffer(&raw, 1);
    let truncated = zlib[..zlib.len() / 2].to_vec();
    let mut mismatch = mesh_buffer(&raw, 0);
    mismatch[6] ^= 1;
    let mut oversize = 0_u16.to_le_bytes().to_vec();
    oversize.extend(u32::MAX.to_le_bytes());
    replace(
        "seeds/rhino_mesh_buffer",
        &[
            ("stored", mesh_buffer(&raw, 0)),
            ("zlib", zlib),
            ("truncated_zlib", truncated),
            ("crc_mismatch", mismatch),
            ("oversize", oversize),
        ],
    );
}

fn generate_brep_seeds() {
    let mut empty = vec![5, 0x30];
    let mut polymorphic_body = 1_i32.to_le_bytes().to_vec();
    polymorphic_body.extend(0_i32.to_le_bytes());
    polymorphic_body.extend(0_i32.to_le_bytes());
    for _ in 0..3 {
        empty.extend(anonymous_chunk(&polymorphic_body));
    }
    let mut packed_body = vec![0x10];
    packed_body.extend(0_i32.to_le_bytes());
    for _ in 0..5 {
        empty.extend(anonymous_chunk(&packed_body));
    }
    push_f64s(&mut empty, &[0.0; 6]);
    let mut truncated = empty.clone();
    truncated.truncate(truncated.len() - 4);
    replace(
        "seeds/rhino_brep",
        &[
            ("empty_valid", empty),
            ("invalid_major", vec![8, 0x40]),
            (
                "oversize_array",
                [vec![5, 0x30], i32::MAX.to_le_bytes().to_vec()].concat(),
            ),
            ("truncated", vec![5]),
            ("truncated_wrapper", truncated),
        ],
    );
}

fn anonymous_chunk(body: &[u8]) -> Vec<u8> {
    crc_chunk(50, 0x4000_0000, body)
}

fn subd_pointer(bytes: &mut Vec<u8>, id: u32, flags: u8) {
    bytes.extend(id.to_le_bytes());
    bytes.push(flags);
}

fn subd_base(bytes: &mut Vec<u8>, archive_id: u32) {
    bytes.extend(archive_id.to_le_bytes());
    bytes.extend((archive_id + 100).to_le_bytes());
    bytes.extend(0_u16.to_le_bytes());
    bytes.extend([0, 0]);
}

fn subd_vertex(bytes: &mut Vec<u8>, archive_id: u32, point: [f64; 3], edges: [u32; 2]) {
    subd_base(bytes, archive_id);
    bytes.push(1);
    push_f64s(bytes, &point);
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.push(0);
    bytes.extend(2_u16.to_le_bytes());
    for edge in edges {
        subd_pointer(bytes, edge, 0x4);
    }
    bytes.extend(1_u16.to_le_bytes());
    subd_pointer(bytes, 9, 0x6);
    bytes.push(0);
}

fn subd_edge(bytes: &mut Vec<u8>, archive_id: u32, vertices: [u32; 2]) {
    subd_base(bytes, archive_id);
    bytes.push(1);
    bytes.extend(1_u16.to_le_bytes());
    push_f64s(bytes, &[0.125, 0.875, 0.25]);
    bytes.extend(2_u16.to_le_bytes());
    for vertex in vertices {
        subd_pointer(bytes, vertex, 0x2);
    }
    bytes.extend(1_u16.to_le_bytes());
    subd_pointer(bytes, 9, 0x6);
    bytes.push(0);
}

fn subd_face(bytes: &mut Vec<u8>) {
    subd_base(bytes, 9);
    bytes.extend(9_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    for edge in 5..=8 {
        subd_pointer(bytes, edge, 0x4);
    }
    bytes.push(0);
}

fn subd_quad() -> Vec<u8> {
    let mut level = Vec::new();
    level.extend(1_i32.to_le_bytes());
    level.extend(1_i32.to_le_bytes());
    level.extend(0_u16.to_le_bytes());
    level.extend([4, 4, 4]);
    push_f64s(&mut level, &[0.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
    for partition in [1_u32, 5, 9, 10] {
        level.extend(partition.to_le_bytes());
    }
    subd_vertex(&mut level, 1, [0.0, 0.0, 0.0], [5, 8]);
    subd_vertex(&mut level, 2, [1.0, 0.0, 0.0], [5, 6]);
    subd_vertex(&mut level, 3, [1.0, 1.0, 0.0], [6, 7]);
    subd_vertex(&mut level, 4, [0.0, 1.0, 0.0], [7, 8]);
    subd_edge(&mut level, 5, [1, 2]);
    subd_edge(&mut level, 6, [2, 3]);
    subd_edge(&mut level, 7, [3, 4]);
    subd_edge(&mut level, 8, [4, 1]);
    subd_face(&mut level);
    level.push(0);

    let mut dimple = Vec::new();
    dimple.extend(1_i32.to_le_bytes());
    dimple.extend(0_i32.to_le_bytes());
    dimple.extend(1_u32.to_le_bytes());
    for value in [9_u32, 9, 9] {
        dimple.extend(value.to_le_bytes());
    }
    push_f64s(&mut dimple, &[0.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
    dimple.extend(anonymous_chunk(&level));

    let mut payload = vec![5, 1];
    payload.extend(anonymous_chunk(&dimple));
    payload
}

fn generate_subd_seeds() {
    let quad = subd_quad();
    let mut truncated_quad = quad.clone();
    truncated_quad.truncate(truncated_quad.len() / 2);
    replace(
        "seeds/rhino_subd",
        &[
            ("empty", vec![5, 0]),
            ("quad", quad),
            ("invalid_presence", vec![8, 2]),
            ("truncated_dimple", vec![8, 1]),
            ("truncated_quad", truncated_quad),
            (
                "oversize",
                [
                    vec![8, 1],
                    0x4000_8000_u32.to_le_bytes().to_vec(),
                    i64::MAX.to_le_bytes().to_vec(),
                ]
                .concat(),
            ),
        ],
    );
}
