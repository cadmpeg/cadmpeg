// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! These helpers hand-build `.prt` byte images and embedded-stream payloads used
//! by the white-box tests relocated into their owning modules and by the golden
//! oracle. They construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::container;

pub(crate) const MAGIC: &[u8; 8] = b"SPLMSSTR";

pub(crate) fn shifted_f64_bytes(value: f64) -> [u8; 8] {
    let mut bytes = value.to_be_bytes();
    bytes[0] -= 0x10;
    bytes
}

pub(crate) fn attach_test_body_surface(
    ir: &mut cadmpeg_ir::document::CadIr,
    body_id: &cadmpeg_ir::ids::BodyId,
    surface: cadmpeg_ir::ids::SurfaceId,
) {
    use cadmpeg_ir::ids::{FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, Region, Sense, Shell};

    let region_id = RegionId(format!("{}:region", body_id.0));
    let shell_id = ShellId(format!("{}:shell", body_id.0));
    if !ir.model.bodies.iter().any(|body| body.id == *body_id) {
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: BodyKind::Solid,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }
    let face_id = FaceId(format!("{}:face#{}", body_id.0, ir.model.faces.len()));
    ir.model
        .shells
        .iter_mut()
        .find(|shell| shell.id == shell_id)
        .unwrap()
        .faces
        .push(face_id.clone());
    ir.model.faces.push(Face {
        id: face_id,
        shell: shell_id,
        surface,
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    });
}

pub(crate) fn be_f64(v: f64) -> [u8; 8] {
    v.to_be_bytes()
}

pub(crate) fn segment_index_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 28] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
    payload
}

pub(crate) fn segment_stream_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (deltas) SCH_test segment stream payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

pub(crate) fn segment_body_binding_payload(stream_kind: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 48, 64, 0, 94, 150, 19, 0] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(64, 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(
            format!(
                "PS\0\0 ({stream_kind}) SCH_test segment body binding payload with more than sixty-four inflated bytes........"
            )
            .as_bytes(),
        )
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

pub(crate) fn segment_extended_wrapper_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 48, 64, 0, 94, 150, 19, 0] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(64, 0);
    payload.extend_from_slice(&0xc000_0005u32.to_le_bytes());
    payload.resize(64 + 38, 0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (partition) SCH_test extended wrapper payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

pub(crate) fn segment_om_payload(separated: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    if separated {
        payload.extend_from_slice(&[0xc0, 0xd1, 0xf1, 0xed]);
    }
    payload.extend_from_slice(&size_framed_om_section());
    payload
}

pub(crate) fn segment_om_record_area_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&size_framed_om_section_with_record_area());
    payload
}

pub(crate) fn multi_section_feature_history_payload() -> Vec<u8> {
    let mut early = size_framed_om_section_with_record_area();
    let name = early
        .windows(b"UNITE".len())
        .position(|window| window == b"UNITE")
        .expect("operation label");
    early[name..name + b"BLOCK".len()].copy_from_slice(b"BLOCK");
    let late = size_framed_om_section_with_record_area();
    let index_byte_len = 36_u32;
    let early_offset = index_byte_len;
    let late_offset = early_offset + early.len() as u32;
    let mut payload = Vec::new();
    for word in [
        late_offset,
        early_offset,
        11,
        1,
        1,
        index_byte_len,
        early_offset,
        9,
        11,
    ] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&early);
    payload.extend_from_slice(&late);
    payload
}

pub(crate) fn segment_om_record_area_with_input_store_payload() -> Vec<u8> {
    let mut payload = segment_om_record_area_payload();
    let mut store = offset_only_indexed_om_section();
    let base = payload.len() as u32;
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    for index in 0..4 {
        let at = index_start + index * 4;
        let value = u32::from_le_bytes(store[at..at + 4].try_into().unwrap());
        store[at..at + 4].copy_from_slice(&(value + base).to_le_bytes());
    }
    payload.extend_from_slice(&store);
    payload
}

/// Append one feature-history operation record (label header + object-index
/// slots + typed payload) to a record area under construction.
pub(crate) fn push_feature_operation(
    bytes: &mut Vec<u8>,
    object_indices: &[u8],
    label: &str,
    payload: &[u8],
) {
    const HEADER: &[u8] = &[
        0x80, 0xcd, 0x01, 0x04, 0x01, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0xff, 0xff,
    ];
    bytes.extend_from_slice(HEADER);
    bytes.extend_from_slice(object_indices);
    bytes.push(0x03);
    bytes.push((label.len() + 2) as u8);
    bytes.extend_from_slice(label.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(payload);
}

/// A feature-history (`UGS::FEATURE_RECORD`) size-framed OM section whose record
/// area packs the supplied operations. `operations` is `(object_index_slots,
/// label, typed_payload)`.
pub(crate) fn composed_feature_history_section(operations: &[(&[u8], &str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = size_framed_om_section();
    let record_area = bytes.len() + 20;
    bytes.extend_from_slice(&(record_area as u32).to_le_bytes());
    bytes.resize(record_area, 0);
    bytes.extend_from_slice(&13u32.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&44u32.to_le_bytes());
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0");
    for (slots, label, payload) in operations {
        push_feature_operation(&mut bytes, slots, label, payload);
    }
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

/// An offset-store indexed OM section carrying `records` as its object-id-less
/// data blocks. The single product record lives in the control block (index 0)
/// so the section validates; `records[i]` resolves to `block#{i + 1}`.
pub(crate) fn composed_offset_store(records: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    let offset_count = records.len() + 2;
    bytes.resize(index_start + offset_count * 4, 0);
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    let mut offsets = Vec::with_capacity(offset_count);
    offsets.push(bytes.len());
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    for record in records {
        offsets.push(bytes.len());
        bytes.extend_from_slice(record);
    }
    offsets.push(bytes.len());
    for (index, offset) in offsets.iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(*offset as u32).to_le_bytes());
    }
    bytes
}

/// Compose a UG_PART payload: segment-index header, one feature-history section
/// with `operations`, and one appended offset store carrying `store_records`.
pub(crate) fn composed_feature_history_payload(
    operations: &[(&[u8], &str, Vec<u8>)],
    store_records: &[&[u8]],
) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&composed_feature_history_section(operations));

    let mut store = composed_offset_store(store_records);
    let base = payload.len() as u32;
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    let offset_count = store_records.len() + 2;
    for index in 0..offset_count {
        let at = index_start + index * 4;
        let value = u32::from_le_bytes(store[at..at + 4].try_into().unwrap());
        store[at..at + 4].copy_from_slice(&(value + base).to_le_bytes());
    }
    payload.extend_from_slice(&store);
    payload
}

pub(crate) type ComposedInputs = (
    Vec<(&'static [u8], &'static str, Vec<u8>)>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
);

/// A 31-character lowercase-hex identity (no `f`, so no `0x66` name markers)
/// shared by the datum-CSYS descriptor in `block3` and the datum-plane
/// descriptor in `block5`, joining them through `datum_plane_csys_identity_uses`.
pub(crate) const COMPOSED_DESCRIPTOR_IDENTITY: &[u8] = b"0123456789abcde0123456789abcde0";

/// Build the operation list and four offset-store data blocks for the composed
/// feature-history fixture.
///
/// - block1+block2 form a two-block offset-store named point `Point7`;
/// - block3+block4 carry rich sketch geometry (named points, scalar fields,
///   coordinate and fixed pairs, and datum-CSYS pair discriminators).
///
/// Operations: `SKETCH` referencing the named point (object indices 1,2),
/// `SKETCH` referencing the geometry (3,4), `DATUM_CSYS` (eight refs to 1) and
/// `DATUM_PLANE`.
pub(crate) fn composed_feature_history_inputs() -> ComposedInputs {
    let sketch_named = vec![
        0x01, 0x00, 0x01, 0x02, 0xf0, 0x01, 0x00, 0x00, 0xf0, 0x02, 0x01, 0x00, 0x00, 0x00,
    ];
    let sketch_geometry = vec![
        0x01, 0x00, 0x01, 0x02, 0xf0, 0x03, 0x00, 0x00, 0xf0, 0x04, 0x01, 0x00, 0x00, 0x00,
    ];
    let mut datum_csys = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for _ in 0..8 {
        datum_csys.extend_from_slice(&[0xf0, 0x03]);
    }
    datum_csys.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    // Single-reference datum-plane branch: descriptor index 5 (block5, a 40-byte
    // descriptor) and object index 3 (block3, the object payload).
    let datum_plane = vec![
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x05, 0x01, 0xf0, 0x03, 0x00,
        0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];

    // Additional container-level operations whose typed payloads each populate a
    // reference/header/lane family directly (mirrors the per-parser white-box
    // fixtures). Object indices in these payloads are intentionally large and do
    // not resolve, so only the leading reference/header arenas populate.
    let point = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa".to_vec();
    let draft = {
        let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49".as_slice();
        let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff".as_slice();
        let terminal =
            b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00"
                .as_slice();
        [prefix, graph, terminal].concat()
    };
    let surface = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02".to_vec();
    let pattern_refs = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02".to_vec();
    let pattern_lane = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01".to_vec();
    let extrude_profile = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00".to_vec();
    let extrude_header =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb"
            .to_vec();
    let extrude_footer = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00".to_vec();
    let block = {
        let mut payload = vec![0x26u8, 0, 0, 1, 0, 0];
        for value in 1..=18u8 {
            payload.extend([0xf0, value]);
        }
        payload.extend([0x01, 0xf1, 0x01, 0x00]);
        payload.extend([0xff; 11]);
        payload.extend([0; 4]);
        payload
    };
    let projected_curve =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0"
            .to_vec();
    // SIMPLE HOLE: two identical scalar runs, each followed by two block-reference
    // tokens, then a canonical `Hole_...` template string.
    let simple_hole = {
        let mut payload = Vec::new();
        payload.extend_from_slice(&shifted_f64_bytes(508.0));
        payload.extend_from_slice(&shifted_f64_bytes(38.1));
        payload.extend_from_slice(&[0xf0, 0x03, 0xf0, 0x04]);
        payload.extend_from_slice(&shifted_f64_bytes(508.0));
        payload.extend_from_slice(&shifted_f64_bytes(38.1));
        payload.extend_from_slice(&[0xf0, 0x03, 0xf0, 0x04]);
        let template = b"Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer";
        payload.extend_from_slice(&[0x04, (template.len() + 2) as u8]);
        payload.extend_from_slice(template);
        payload.push(0x00);
        payload
    };

    let operations: Vec<(&'static [u8], &'static str, Vec<u8>)> = vec![
        (&[1, 0xff, 0xff, 0xff], "SKETCH", sketch_named),
        (&[3, 0xff, 0xff, 0xff], "SKETCH", sketch_geometry),
        (&[3, 0xff, 0xff, 0xff], "DATUM_CSYS", datum_csys),
        (&[3, 0xff, 0xff, 0xff], "DATUM_PLANE", datum_plane),
        (&[3, 0xff, 0xff, 0xff], "POINT", point),
        (&[3, 0xff, 0xff, 0xff], "DRAFT", draft),
        (&[3, 0xff, 0xff, 0xff], "SKIN", surface),
        (&[3, 0xff, 0xff, 0xff], "Geometry Instance", pattern_refs),
        (&[3, 0xff, 0xff, 0xff], "Pattern Feature", pattern_lane),
        (&[3, 0xff, 0xff, 0xff], "EXTRUDE", extrude_profile),
        (&[3, 0xff, 0xff, 0xff], "EXTRUDE", extrude_header),
        (&[3, 0xff, 0xff, 0xff], "EXTRUDE", extrude_footer),
        (&[3, 0xff, 0xff, 0xff], "BLOCK", block),
        (&[3, 0xff, 0xff, 0xff], "CPROJ", projected_curve),
        (&[3, 0xff, 0xff, 0xff], "SIMPLE HOLE", simple_hole),
    ];

    // Two-block offset-store named point `Point7` (leading name + scalar in
    // block1, the second scalar in block2).
    let mut block1: Vec<u8> = Vec::new();
    block1.extend_from_slice(&[0x03, 0x08]);
    block1.extend_from_slice(b"Point7");
    block1.push(0x00);
    block1.extend_from_slice(&[
        0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ]);
    let block2: Vec<u8> = vec![
        0x50, 0x59, 0x66, 0x59, 0x00, 0x31, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];

    // Rich sketch geometry across block3 (payload) and block4 (terminal filler).
    let mut block3: Vec<u8> = Vec::new();
    // Point1: payload-leading name plus two PYf scalar fields.
    block3.extend_from_slice(&[0x03, 0x08]);
    block3.extend_from_slice(b"Point1");
    block3.push(0x00);
    block3.extend_from_slice(&[
        0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ]);
    block3.extend_from_slice(&[
        0x50, 0x59, 0x66, 0x59, 0x00, 0x31, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ]);
    // Point2: 66-form name plus one signed Q1.55 fixed pair (no scalars).
    block3.extend_from_slice(&[0x66, 0x32, 0x03, 0x08]);
    block3.extend_from_slice(b"Point2");
    block3.push(0x00);
    block3.extend_from_slice(&[
        0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    // Point3: 66-form name closing Point2's named-record interval.
    block3.extend_from_slice(&[0x66, 0x32, 0x03, 0x08]);
    block3.extend_from_slice(b"Point3");
    block3.push(0x00);
    // Coordinate pair (object_payload_scalar_pairs SHORT discriminator).
    block3.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ]);
    // datum_csys signed Q1.55 fixed pair (0b discriminator).
    block3.extend_from_slice(&[
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ]);
    // datum_plane object scalar pair (6d 00 f0 + coordinate discriminator).
    block3.extend_from_slice(&[0x6d, 0x00, 0xf0]);
    block3.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ]);
    // datum_csys descriptor identity: a maximal 31-char hex run bounded by nulls.
    block3.push(0x00);
    block3.extend_from_slice(COMPOSED_DESCRIPTOR_IDENTITY);
    block3.push(0x00);
    let block4: Vec<u8> = vec![0x00];

    // block5: a 40-byte datum-plane descriptor block sharing the CSYS identity.
    let mut block5: Vec<u8> = Vec::new();
    block5.extend_from_slice(COMPOSED_DESCRIPTOR_IDENTITY); // hex identity (31)
    block5.extend_from_slice(b"?A"); // delimiter + form marker
    block5.push(0x03); // compact schema index
    block5.extend_from_slice(&[0xff, 0x02, 0x01]); // fixed separator
    block5.extend_from_slice(b"DPd"); // graphic label; pads block to 40 bytes
    debug_assert_eq!(block5.len(), 40);

    (operations, block1, block2, block3, block4, block5)
}

/// A `.prt` image whose single feature-history section and companion offset
/// store drive the feature-history arena families that no other golden reaches:
/// the complete sketch family (records, references, construction inputs and
/// payloads, coordinate/fixed pairs, scalars, names, named records, points,
/// fixed points, point groups, named-point/preceding/point uses, and the
/// datum-CSYS dependency), the datum-CSYS and datum-plane families (constructions,
/// payloads, pairs, scalars, descriptors, headers, block uses, identity uses),
/// plus the point/draft/surface/pattern/extrude/block reference and header lanes.
pub(crate) fn composed_feature_history_prt() -> Vec<u8> {
    let (operations, block1, block2, block3, block4, block5) = composed_feature_history_inputs();
    let store_records: Vec<&[u8]> = vec![&block1, &block2, &block3, &block4, &block5];
    let payload = composed_feature_history_payload(&operations, &store_records);
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

/// Write three big-endian doubles into `rec` starting at `at`.
pub(crate) fn put_vec3(rec: &mut [u8], at: usize, xyz: [f64; 3]) {
    for (i, v) in xyz.iter().enumerate() {
        rec[at + 8 * i..at + 8 * i + 8].copy_from_slice(&be_f64(*v));
    }
}

pub(crate) fn put_f64(rec: &mut [u8], at: usize, v: f64) {
    rec[at..at + 8].copy_from_slice(&be_f64(v));
}

pub(crate) fn put_ref(rec: &mut [u8], at: usize, value: u16) {
    rec[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn encoded_xmt(value: u32) -> Vec<u8> {
    if i16::try_from(value).is_ok() {
        return (value as u16).to_be_bytes().to_vec();
    }
    let quotient = value / 32_767;
    let remainder = value % 32_767;
    assert!(remainder > 0 && i16::try_from(remainder).is_ok());
    let mut out = (-(remainder as i16)).to_be_bytes().to_vec();
    out.extend_from_slice(&(quotient as u16).to_be_bytes());
    out
}

/// One fixed-length analytic record: a `00 <tag>` header then zeroed payload the
/// caller fills at the documented offsets.
pub(crate) fn record(tag: u8, len: usize) -> Vec<u8> {
    let mut r = vec![0u8; len];
    r[0] = 0x00;
    r[1] = tag;
    r
}

pub(crate) fn indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 32];
    let base = 8usize;
    let class_name = b"UGS::EXP_expression";
    bytes[base] = (class_name.len() + 1) as u8;
    bytes[base + 1..base + 1 + class_name.len()].copy_from_slice(class_name);
    bytes[base + 1 + class_name.len()] = 0x81;
    let field_name = b"m_target";
    bytes.push((field_name.len() + 1) as u8);
    bytes.extend_from_slice(field_name);
    bytes.push(0x80);
    let root = b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables";
    let text = b"(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; ";
    let declaration_name = b"p8_CircularPattern_pattern_Circular_Dir_offset_angle";
    let mut expression = vec![0x04, (declaration_name.len() + 2) as u8];
    expression.extend_from_slice(declaration_name);
    expression.push(0);
    expression.extend_from_slice(b"\x04\x05120\0");
    expression.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    expression.extend_from_slice(text);
    expression.push(0);
    expression.extend_from_slice(b"\x66\x32\x03\x0cSKETCH_001\0");
    expression.extend_from_slice(b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0");
    expression.extend_from_slice(b"\x01\x02\x90\x00\x00");
    let records = [root.as_slice(), expression.as_slice()];
    let table = bytes.len() + 4 * 4;
    let table_end = table + 4 + 3 * 4;
    let first = table_end - base;
    let second = first + records[0].len();
    let end = second + records[1].len();
    for value in [0u32, first as u32, second as u32, end as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for id in [0x100u32, 0x101, 0x102] {
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes.extend_from_slice(records[0]);
    bytes.extend_from_slice(records[1]);
    bytes
}

pub(crate) fn offset_only_indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 0]);
    let first = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0hostglobalvariables");
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

/// An offset-store indexed OM section whose control block is replaced by
/// `control_block`. The first record remains the single supported product
/// record (so the section validates), leaving the control block free to carry
/// persistent-handle references or an index-value array for the
/// `data_block_control_*` extractors.
pub(crate) fn offset_only_indexed_om_section_with_control(control_block: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(control_block);
    let first = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0hostglobalvariables");
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

/// An offset-store indexed OM section whose single product record lives inside
/// the control block, preceded by a zero-prefixed aligned index-value array.
/// The two column records carry no product marker, so the section still holds
/// exactly one product record and `data_block_control_index_values` decodes the
/// array. Mirrors the `om_offset_store_index_values_*` white-box test.
pub(crate) fn offset_only_indexed_om_section_with_index_values() -> Vec<u8> {
    let mut control = Vec::new();
    control.extend_from_slice(&[0, 0]); // two-byte zero prefix
    control.extend_from_slice(&7u32.to_le_bytes());
    control.extend_from_slice(&0x1020u32.to_le_bytes());
    control.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0"); // the one product record

    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(&control);
    let first = bytes.len();
    bytes.extend_from_slice(&[0xbb; 12]); // column record, no product marker
    let second = bytes.len();
    bytes.extend_from_slice(&[0xcc; 12]); // column record, no product marker
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

/// An offset-store indexed OM section whose first (object-id-less) record is an
/// offset-store named point (`Point7` with two `57.15` scalars). The single
/// product record lives in the control block so the section validates while the
/// column records carry the point payload. Mirrors the
/// `om_offset_store_named_point_*` white-box test.
pub(crate) fn offset_only_indexed_om_section_with_named_point() -> Vec<u8> {
    let mut named_point = vec![
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'7', 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30,
        0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    named_point.extend_from_slice(&[
        0x45, 0x04, 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33,
        0x07,
    ]);

    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0"); // product record in control block
    let first = bytes.len();
    bytes.extend_from_slice(&named_point); // first record: the named point
    let second = bytes.len();
    bytes.extend_from_slice(&[0xbb; 8]); // trailing column record, no point payload
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

pub(crate) fn control_root_offset_only_indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let control = bytes.len();
    bytes.extend_from_slice(&[0xf0, 1, 0, 0]);
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0control-tail");
    let first = bytes.len();
    bytes.extend_from_slice(&[0; 32]);
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(b"hostglobalvariables");
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [control, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

pub(crate) fn size_framed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xff; 16];
    bytes[4..8].fill(0);
    bytes[12..14].copy_from_slice(b"OM");
    bytes.extend_from_slice(&[0, 1, 2]);
    for (index, (name, code)) in [
        (b"UGS::FEATURE_RECORD".as_slice(), 0xa0),
        (b"UGS::ModlUtils::BooleanComponent".as_slice(), 0x65),
    ]
    .into_iter()
    .enumerate()
    {
        bytes.push((name.len() + 1) as u8);
        bytes.extend_from_slice(name);
        bytes.push(code);
        if index == 0 {
            bytes.extend_from_slice(&[
                0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ]);
        }
    }
    for (name, code, suffix) in [
        (b"m_target".as_slice(), 0x80, [0x01, 0x02]),
        (b"m_tools".as_slice(), 0x81, [0x03, 0x04]),
    ] {
        bytes.push((name.len() + 1) as u8);
        bytes.extend_from_slice(name);
        bytes.push(code);
        bytes.extend_from_slice(&suffix);
    }
    bytes.extend_from_slice(b"unframed UGS::PayloadText");
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

pub(crate) fn size_framed_om_section_with_record_area() -> Vec<u8> {
    let mut bytes = size_framed_om_section();
    let record_area = bytes.len() + 20;
    bytes.extend_from_slice(&(record_area as u32).to_le_bytes());
    bytes.resize(record_area, 0);
    bytes.extend_from_slice(&13u32.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&44u32.to_le_bytes());
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0feature-records\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x42\x00\x01\x03\x90\x19\x4c\x7f\x00\x01\x02\x10\x90\x19\x42\xff");
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

pub(crate) fn size_framed_om_section_with_repeated_operations(count: usize) -> Vec<u8> {
    let section = size_framed_om_section_with_record_area();
    let operation = section
        .windows(15)
        .position(|window| {
            window == b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff"
        })
        .expect("operation marker");
    let mut bytes = section[..operation].to_vec();
    for _ in 0..count {
        bytes.extend_from_slice(&section[operation..]);
    }
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

/// A synthetic Parasolid partition stream: the `PS 00 00` header, a prologue with
/// a `(partition)` subtype and a schema token, then one POINT, one PLANE, one
/// CYLINDER, and one LINE record laid out back-to-back at their fixed lengths.
pub(crate) fn partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00");
    s.extend_from_slice(b"SCH_TEST_1_9999\x00");

    // POINT (type 29): xyz at +16, metres.
    let mut pt = record(0x1d, 40);
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]); // 62.5, 0, 12.7 mm
    s.extend_from_slice(&pt);

    // PLANE (type 50): origin +19, normal +43, x_axis +67.
    let mut pl = record(0x32, 91);
    pl[18] = b'+';
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]); // 76.2 mm
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    // CYLINDER (type 51): origin +19, axis +43, radius +67, x_axis +75.
    let mut cy = record(0x33, 99);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05); // 4.05 mm
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    // LINE (type 30): point +19, direction +43.
    let mut ln = record(0x1e, 67);
    ln[18] = b'+';
    put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&ln);

    s
}

/// Raw bytes for an `ExternalReferences` container entry: an `EXTREFSTREAM`
/// index over one empty record and one four-slot handle-set record, followed by
/// an end-anchored four-string table. Decoding walks the record index, string
/// table, empty-record form, handle-set slots, and the handle/tagged tail,
/// populating every `external_reference*` arena. The record and table byte
/// shapes mirror the `external_reference_*` white-box tests.
pub(crate) fn external_reference_stream() -> Vec<u8> {
    let mut p = b"EXTREFSTREAM".to_vec();
    p.extend_from_slice(&[0u8; 13]); // header; byte 24 must be zero
    debug_assert_eq!(p.len(), 25);
    // Record directory (ascending offsets): empty record 7 at 45, handle-set 6 at 51.
    p.extend_from_slice(&7u32.to_le_bytes());
    p.extend_from_slice(&45u32.to_le_bytes());
    p.extend_from_slice(&6u32.to_le_bytes());
    p.extend_from_slice(&51u32.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes()); // terminator
    debug_assert_eq!(p.len(), 45);
    // Empty record 7: the exact six-byte form.
    p.extend_from_slice(&[1, 0, 0, 0, 0, 1]);
    debug_assert_eq!(p.len(), 51);
    // Handle-set record 6.
    p.extend_from_slice(&[1, 0, 0, 0]); // record marker
    p.extend_from_slice(&2u16.to_be_bytes()); // declared count
    p.push(1);
    for slot in [0u32, 1, 2, 3] {
        p.extend_from_slice(&slot.to_le_bytes()); // id slots
    }
    p.push(1); // record[23]
    p.push(3); // record[24] = token count
    p.extend_from_slice(&[0xe0, 0, 0, 0, 0x10]); // ascending handles
    p.extend_from_slice(&[0xe0, 0, 0, 0, 0x20]);
    p.push(3); // prefix closing count
               // Tail: one adjacent persistent-handle / tagged-reference pair.
    p.extend_from_slice(&[0xe0, 0, 0, 0, 0x05, 0xc0, 0, 0, 0x01]);
    debug_assert_eq!(p.len(), 96);
    // End-anchored string table: four strings, ordinals 0..3.
    p.push(1);
    p.extend_from_slice(&4u32.to_le_bytes());
    for value in ["child.prt", "dirA", "dirB", "extra"] {
        p.extend_from_slice(&(value.len() as u16).to_le_bytes());
        p.extend_from_slice(value.as_bytes());
    }
    p
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry: a one-row outer
/// index pointing at a single embedded JT 9.4 document whose table of contents
/// declares one compressed segment. Decoding walks
/// `display_jt_indices -> display_jt_documents -> display_jt_segments ->
/// display_jt_compressed_element_sequences`, populating those arenas plus
/// `display_jt_compressed_elements`. The byte layout mirrors the
/// `display_jt_index_requires_every_declared_header` white-box test.
pub(crate) fn display_jt_basic_stream() -> Vec<u8> {
    let mut inflated = Vec::new();
    inflated.extend_from_slice(&24_u32.to_le_bytes());
    inflated.extend_from_slice(&[3; 16]);
    inflated.push(1);
    inflated.extend_from_slice(&5_u32.to_le_bytes());
    inflated.extend_from_slice(&[9, 8, 7]);
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&[6, 5]);
    let compressed = zlib_compress_at_level(&inflated, 1);
    let segment_byte_len = 24 + 9 + compressed.len() as u32;

    let mut data = Vec::new();
    // Outer index: version 9, one row.
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes()); // word-swapped value
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes()); // header offset
    data.extend_from_slice(&[0; 4]);
    // Embedded JT document header at offset 28.
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0); // byte order
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes()); // toc offset
    data.extend_from_slice(&[1; 16]); // lsg segment id
                                      // Table of contents at offset 105: one entry.
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]); // segment id
    data.extend_from_slice(&137_u32.to_le_bytes()); // segment offset
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes()); // attribute (segment type 1)
                                                  // Segment at offset 137.
    data.extend_from_slice(&[2; 16]); // segment id
    data.extend_from_slice(&1_u32.to_le_bytes()); // segment type
    data.extend_from_slice(&segment_byte_len.to_le_bytes()); // header byte len
    data.extend_from_slice(&2_u32.to_le_bytes()); // compression flag
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2); // algorithm
    data.extend_from_slice(&compressed);
    data
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry whose single type-7
/// shape-LOD segment frames one tri-strip LOD element. The element's base type,
/// object-type UUID, and tri-strip LOD header body decode
/// `display_jt_shape_lod_elements` and `display_jt_tri_strip_lod_headers`.
pub(crate) fn display_jt_shape_lod_stream() -> Vec<u8> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // Tri-strip LOD header body: fixed version/binding fields then a short
    // compressed-representation tail (only hashed, not decoded, by the header).
    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_le_bytes()); // base version
    body.extend_from_slice(&1u16.to_le_bytes()); // vertex version
    body.extend_from_slice(&0u64.to_le_bytes()); // vertex bindings
    body.extend_from_slice(&1u16.to_le_bytes()); // topological mesh version
    body.extend_from_slice(&0u32.to_le_bytes()); // vertex records object id
    body.extend_from_slice(&1u16.to_le_bytes()); // compressed LOD version
    body.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]); // representation tail

    let element = jt_scene_element(TRI_STRIP_LOD_TYPE, 4, 42, &body);
    let mut payload = element;
    payload.extend_from_slice(&16u32.to_le_bytes());
    payload.extend_from_slice(&[0xff; 16]);
    payload.extend_from_slice(&[1, 0, 0, 0, 0, 0]); // segment tail

    let segment_byte_len = 24 + payload.len() as u32;
    let mut segment = Vec::new();
    segment.extend_from_slice(&[2; 16]); // segment id
    segment.extend_from_slice(&7u32.to_le_bytes()); // segment type
    segment.extend_from_slice(&segment_byte_len.to_le_bytes()); // header byte len
    segment.extend_from_slice(&payload);

    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&7_u32.to_be_bytes()); // attribute type 7
    data.extend_from_slice(&segment);
    data
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry whose single type-31
/// property segment inflates to one string-property atom, decoding
/// `display_jt_string_property_atoms`.
pub(crate) fn display_jt_string_property_stream() -> Vec<u8> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // String atom body: the fixed prefix, a UTF-16 length, then the code units.
    let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    body.extend_from_slice(&2u32.to_le_bytes());
    for unit in "JT".encode_utf16() {
        body.extend_from_slice(&unit.to_le_bytes());
    }

    let mut inflated = jt_scene_element(STRING_PROPERTY_ATOM_TYPE, 5, 1, &body);
    inflated.extend_from_slice(&16u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let compressed = zlib_compress_at_level(&inflated, 1);
    let segment_byte_len = 24 + 9 + compressed.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&31_u32.to_be_bytes()); // attribute type 31
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&31_u32.to_le_bytes()); // segment type 31
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&2_u32.to_le_bytes());
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2);
    data.extend_from_slice(&compressed);
    data
}

/// Frame one JT logical element: length-prefixed `[type_id][base_type][object_id]
/// [body]`, matching `parse_jt_element_sequence`.
pub(crate) fn jt_scene_element(
    type_id: [u8; 16],
    base_type: u8,
    object_id: u32,
    body: &[u8],
) -> Vec<u8> {
    let mut element = Vec::new();
    let byte_len = 16 + 1 + 4 + body.len();
    element.extend_from_slice(&(byte_len as u32).to_le_bytes());
    element.extend_from_slice(&type_id);
    element.push(base_type);
    element.extend_from_slice(&object_id.to_le_bytes());
    element.extend_from_slice(body);
    element
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry whose single type-1
/// scene-graph segment inflates to an element sequence of one instance, group,
/// partition, range-LOD, tri-strip shape, and geometric-transform node. Each
/// node's object-type UUID, base type, and body match the corresponding
/// `display_jt9_*` white-box test, so decoding populates
/// `display_jt_base_node_data`, `_group_node_data`, `_instance_nodes`,
/// `_partition_nodes`, `_range_lod_nodes`, `_tri_strip_shape_nodes`, and
/// `_geometric_transform_attributes`.
pub(crate) fn display_jt_scene_graph_stream() -> Vec<u8> {
    const INSTANCE: [u8; 16] = [
        0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const PARTITION: [u8; 16] = [
        0x3e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const RANGE_LOD: [u8; 16] = [
        0x4c, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const TRI_STRIP_SHAPE: [u8; 16] = [
        0x77, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const GEOMETRIC_TRANSFORM: [u8; 16] = [
        0x83, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // A group node whose object-type UUID matches no specialized scene node, so
    // only the base-node and group-node extractors decode it.
    const GROUP: [u8; 16] = [0x11; 16];

    // Instance node body: base header (attribute id 7) then a one-child family.
    let mut instance = Vec::new();
    instance.extend_from_slice(&1u16.to_le_bytes());
    instance.extend_from_slice(&0x20u32.to_le_bytes());
    instance.extend_from_slice(&1u32.to_le_bytes());
    instance.extend_from_slice(&7u32.to_le_bytes());
    instance.extend_from_slice(&1u16.to_le_bytes());
    instance.extend_from_slice(&9u32.to_le_bytes());

    // Group node body: base header (no attributes) then ordered children.
    let mut group = Vec::new();
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&0u32.to_le_bytes());
    group.extend_from_slice(&0u32.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&2u32.to_le_bytes());
    group.extend_from_slice(&7u32.to_le_bytes());
    group.extend_from_slice(&9u32.to_le_bytes());
    group.extend_from_slice(&[4, 3, 2, 1]);

    // Partition node body.
    let mut partition = Vec::new();
    partition.extend_from_slice(&1u16.to_le_bytes());
    partition.extend_from_slice(&0u32.to_le_bytes());
    partition.extend_from_slice(&0u32.to_le_bytes());
    partition.extend_from_slice(&1u16.to_le_bytes());
    partition.extend_from_slice(&1u32.to_le_bytes());
    partition.extend_from_slice(&2u32.to_le_bytes());
    partition.extend_from_slice(&1u32.to_le_bytes());
    partition.extend_from_slice(&1u32.to_le_bytes());
    partition.extend_from_slice(&u16::from(b'x').to_le_bytes());
    for value in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        partition.extend_from_slice(&value.to_le_bytes());
    }
    partition.extend_from_slice(&6.0f32.to_le_bytes());
    for value in [1i32, 2, 3, 4, 5, 6] {
        partition.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        partition.extend_from_slice(&value.to_le_bytes());
    }

    // Range-LOD node body.
    let mut range = Vec::new();
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&0u32.to_le_bytes());
    range.extend_from_slice(&0u32.to_le_bytes());
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&2u32.to_le_bytes());
    range.extend_from_slice(&7u32.to_le_bytes());
    range.extend_from_slice(&9u32.to_le_bytes());
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&1u32.to_le_bytes());
    range.extend_from_slice(&0.25f32.to_le_bytes());
    range.extend_from_slice(&(-2i32).to_le_bytes());
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&2u32.to_le_bytes());
    range.extend_from_slice(&10.0f32.to_le_bytes());
    range.extend_from_slice(&20.0f32.to_le_bytes());
    for value in [1.0f32, 2.0, 3.0] {
        range.extend_from_slice(&value.to_le_bytes());
    }

    // Tri-strip shape node body.
    let mut tri_strip = Vec::new();
    tri_strip.extend_from_slice(&1u16.to_le_bytes());
    tri_strip.extend_from_slice(&0x20u32.to_le_bytes());
    tri_strip.extend_from_slice(&0u32.to_le_bytes());
    tri_strip.extend_from_slice(&1u16.to_le_bytes());
    for value in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        tri_strip.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        tri_strip.extend_from_slice(&value.to_le_bytes());
    }
    tri_strip.extend_from_slice(&6.0f32.to_le_bytes());
    for value in [7i32, 8, 9, 10, 11, 12] {
        tri_strip.extend_from_slice(&value.to_le_bytes());
    }
    tri_strip.extend_from_slice(&4096u32.to_le_bytes());
    tri_strip.extend_from_slice(&0.75f32.to_le_bytes());
    tri_strip.extend_from_slice(&2u16.to_le_bytes());
    tri_strip.extend_from_slice(&0x102u64.to_le_bytes());
    tri_strip.extend_from_slice(&[24, 13, 16, 8]);
    tri_strip.extend_from_slice(&0x304u64.to_le_bytes());

    // Geometric-transform attribute body (sparse affine matrix).
    let mut geom = Vec::new();
    geom.extend_from_slice(&1u16.to_le_bytes());
    geom.push(0x08);
    geom.extend_from_slice(&0u32.to_le_bytes());
    geom.extend_from_slice(&1u16.to_le_bytes());
    geom.extend_from_slice(&0x000eu16.to_le_bytes());
    for value in [1.25f32, -2.5, 4.0] {
        geom.extend_from_slice(&value.to_le_bytes());
    }

    let mut inflated = Vec::new();
    inflated.extend_from_slice(&jt_scene_element(INSTANCE, 0, 1, &instance));
    inflated.extend_from_slice(&jt_scene_element(GROUP, 1, 2, &group));
    inflated.extend_from_slice(&jt_scene_element(PARTITION, 1, 3, &partition));
    inflated.extend_from_slice(&jt_scene_element(RANGE_LOD, 1, 4, &range));
    inflated.extend_from_slice(&jt_scene_element(TRI_STRIP_SHAPE, 2, 5, &tri_strip));
    inflated.extend_from_slice(&jt_scene_element(GEOMETRIC_TRANSFORM, 3, 6, &geom));
    // End-of-sequence marker.
    inflated.extend_from_slice(&16u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let compressed = zlib_compress_at_level(&inflated, 1);
    let segment_byte_len = 24 + 9 + compressed.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&2_u32.to_le_bytes());
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2);
    data.extend_from_slice(&compressed);
    data
}

/// A Parasolid `(partition)` stream carrying the neutral-binary attribute and
/// typed-entity records (`00 4f`/`00 50` class declaration, `00 51` framed
/// entity, `00 52`/`00 53` counted value records, `00 54` string record) whose
/// extractors are exercised by the `parasolid_entity_*`, `parasolid_attribute_*`
/// white-box tests. The `00 51` entity's references resolve to the value and
/// string records, and its discriminator selects the class declaration, so the
/// join arenas (`parasolid_entity_51_numeric_uses`, `parasolid_entity_51_string_uses`,
/// `parasolid_attribute_class_uses`) are populated as well.
pub(crate) fn parasolid_entity_records_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    // `00 4f` attribute-class declaration with identity xmt 201, followed by its
    // `00 50` field record (one field). A `00 51` entity with discriminator 200
    // resolves to this class through `definition_xmt = discriminator + 1`.
    s.extend_from_slice(&[0x00, 0x4f]);
    s.extend_from_slice(&10u32.to_be_bytes()); // name length
    s.extend_from_slice(&201u16.to_be_bytes()); // class identity xmt
    s.extend_from_slice(b"ATTR_CLASS");
    s.extend_from_slice(&[0x00, 0x50]); // field-record tag
    s.extend_from_slice(&1u32.to_be_bytes()); // field count
    s.extend_from_slice(&202u16.to_be_bytes()); // field-record xmt
    s.extend_from_slice(&0u16.to_be_bytes()); // reference 0
    s.extend_from_slice(&0u16.to_be_bytes()); // reference 1
    s.extend_from_slice(&0u16.to_be_bytes()); // header word 0
    s.extend_from_slice(&0u16.to_be_bytes()); // header word 1
    s.extend_from_slice(&[0xaa; 26]); // 26-byte descriptor prefix
    s.push(0x01); // one field code

    // `00 52` counted unsigned-integer record, identity xmt 101, one value.
    s.extend_from_slice(&[0x00, 0x52]);
    s.extend_from_slice(&1u32.to_be_bytes()); // count
    s.extend_from_slice(&101u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&7u32.to_be_bytes()); // value

    // `00 53` counted binary64 record, identity xmt 102, one finite value.
    s.extend_from_slice(&[0x00, 0x53]);
    s.extend_from_slice(&1u32.to_be_bytes()); // count
    s.extend_from_slice(&102u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&1.5f64.to_be_bytes()); // value

    // `00 54` printable string record, identity xmt 100.
    s.extend_from_slice(&[0x00, 0x54]);
    s.extend_from_slice(&10u32.to_be_bytes()); // length
    s.extend_from_slice(&100u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(b"ATTR_LABEL");
    s.push(0x00); // terminator

    // `00 51` framed entity: flags 1 (low_flag 1 -> six references), identity
    // xmt 50, sequence 2, discriminator 200. Its references resolve to the
    // string (100), integer (101), and double (102) records above.
    s.extend_from_slice(&[0x00, 0x51]);
    s.extend_from_slice(&1u32.to_be_bytes()); // flags
    s.extend_from_slice(&50u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&2u32.to_be_bytes()); // sequence
    s.extend_from_slice(&200u16.to_be_bytes()); // discriminator
    for reference in [100u16, 101, 102, 150, 151, 152] {
        s.extend_from_slice(&reference.to_be_bytes());
    }
    s.extend_from_slice(&[0xaa, 0xaa]); // trailing padding

    s
}

/// A complete one-face Parasolid topology. Every ownership and geometry link is
/// a small XMT reference, so this generated fixture exercises the codec's
/// connected-B-rep path without depending on an external CAD file.
pub(crate) fn topology_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    s.extend_from_slice(&body);

    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    put_ref(&mut shell, 8, 1); // attributes
    put_ref(&mut shell, 10, 2); // body
    put_ref(&mut shell, 12, 1); // next shell
    put_ref(&mut shell, 14, 4); // first face
    put_ref(&mut shell, 16, 1); // sentinel
    put_ref(&mut shell, 18, 1); // sentinel
    put_ref(&mut shell, 20, 12); // region
    put_ref(&mut shell, 22, 1); // sentinel
    s.extend_from_slice(&shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    put_f64(&mut face, 10, 0.000_2); // 0.2 mm
    put_ref(&mut face, 18, 1); // next face
    put_ref(&mut face, 20, 1); // previous face
    put_ref(&mut face, 22, 5); // loop
    put_ref(&mut face, 24, 3); // shell
    put_ref(&mut face, 26, 6); // plane
    face[28] = b'+';
    s.extend_from_slice(&face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 5);
    put_ref(&mut loop_, 10, 7); // fin
    put_ref(&mut loop_, 12, 4); // face
    put_ref(&mut loop_, 14, 1); // next loop
    s.extend_from_slice(&loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 7);
    put_ref(&mut fin, 6, 5); // loop
    put_ref(&mut fin, 8, 7); // next (one-fin ring)
    put_ref(&mut fin, 10, 7); // previous
    put_ref(&mut fin, 12, 10); // vertex
    put_ref(&mut fin, 14, 1); // no partner fin
    put_ref(&mut fin, 16, 8); // edge
    put_ref(&mut fin, 18, 9); // curve
    fin[22] = b'+';
    s.extend_from_slice(&fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 8);
    put_f64(&mut edge, 10, 0.000_3); // 0.3 mm
    put_ref(&mut edge, 18, 7); // fin
    put_ref(&mut edge, 24, 9); // curve
    s.extend_from_slice(&edge);

    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 6);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&plane);

    let mut line = record(30, 67);
    put_ref(&mut line, 2, 9);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&line);

    let mut vertex = record(18, 28);
    put_ref(&mut vertex, 2, 10);
    put_ref(&mut vertex, 16, 11); // point
    put_f64(&mut vertex, 18, 0.000_1); // 0.1 mm
    s.extend_from_slice(&vertex);

    let mut region = record(19, 16);
    put_ref(&mut region, 2, 12);
    s.extend_from_slice(&region);

    let mut point = record(29, 40);
    put_ref(&mut point, 2, 11);
    put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
    s.extend_from_slice(&point);
    s
}

pub(crate) fn offset_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);

    let mut offset = record(60, 31);
    put_ref(&mut offset, 2, 12);
    offset[18] = b'+';
    offset[19] = b'V';
    offset[20] = 1;
    put_ref(&mut offset, 21, 6);
    put_f64(&mut offset, 23, 0.002_5);
    stream.extend(offset);
    stream
}

pub(crate) fn surface_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

pub(crate) fn pcurve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 25);
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.01, 0.02, 0.0]);

    let mut wrapper = record(134, 23);
    put_ref(&mut wrapper, 2, 20);
    wrapper[18] = b'+';
    put_ref(&mut wrapper, 19, 21);
    put_ref(&mut wrapper, 21, 22);
    stream.extend(wrapper);

    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 21);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 2);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 23);
    put_ref(&mut descriptor, 25, 24);
    stream.extend(descriptor);

    let mut payload = record(135, 15 + 4 * 8);
    put_ref(&mut payload, 2, 22);
    payload[9..13].copy_from_slice(&4u32.to_be_bytes());
    for (index, value) in [0.01, 0.02, 0.01, 0.02].into_iter().enumerate() {
        put_f64(&mut payload, 15 + index * 8, value);
    }
    stream.extend(payload);

    let mut multiplicities = record(127, 12);
    multiplicities[4..6].copy_from_slice(&2u16.to_be_bytes());
    put_ref(&mut multiplicities, 6, 23);
    put_ref(&mut multiplicities, 8, 2);
    put_ref(&mut multiplicities, 10, 2);
    stream.extend(multiplicities);

    let mut knots = record(128, 24);
    knots[4..6].copy_from_slice(&2u16.to_be_bytes());
    put_ref(&mut knots, 6, 24);
    put_f64(&mut knots, 8, 0.0);
    put_f64(&mut knots, 16, 1.0);
    stream.extend(knots);

    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 25);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 20);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

pub(crate) fn shared_region_shells_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 13);
    for (offset, reference) in [
        (8, 1),
        (10, 2),
        (12, 1),
        (14, 14),
        (16, 1),
        (18, 1),
        (20, 12),
        (22, 1),
    ] {
        put_ref(&mut shell, offset, reference);
    }
    stream.extend(shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 14);
    put_f64(&mut face, 10, 0.000_2);
    put_ref(&mut face, 18, 1);
    put_ref(&mut face, 20, 1);
    put_ref(&mut face, 22, 15);
    put_ref(&mut face, 24, 13);
    put_ref(&mut face, 26, 6);
    face[28] = b'+';
    stream.extend(face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 15);
    put_ref(&mut loop_, 10, 16);
    put_ref(&mut loop_, 12, 14);
    put_ref(&mut loop_, 14, 1);
    stream.extend(loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 16);
    put_ref(&mut fin, 6, 15);
    put_ref(&mut fin, 8, 16);
    put_ref(&mut fin, 10, 16);
    put_ref(&mut fin, 12, 10);
    put_ref(&mut fin, 14, 1);
    put_ref(&mut fin, 16, 17);
    put_ref(&mut fin, 18, 9);
    fin[22] = b'+';
    stream.extend(fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 17);
    put_f64(&mut edge, 10, 0.000_3);
    put_ref(&mut edge, 18, 16);
    put_ref(&mut edge, 24, 9);
    stream.extend(edge);
    stream
}

pub(crate) fn blend_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);

    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.003);
    put_f64(&mut blend, 34, 0.003);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    stream.extend(blend);
    stream
}

pub(crate) fn blend_surface_with_extended_support_reference() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    stream.splice(blend + 20..blend + 22, [0xff, 0xfa, 0x00, 0x00]);
    stream
}

pub(crate) fn blend_surface_with_intersection_spine() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_ref(&mut stream, blend + 24, 18);

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 18);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

pub(crate) fn blend_surface_with_forward_blend_support() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("first blend record");
    put_ref(&mut stream, first + 20, 20);

    let mut second = record(56, 66);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    second[19] = b'R';
    put_ref(&mut second, 20, 6);
    put_ref(&mut second, 22, 6);
    put_ref(&mut second, 24, 1);
    put_f64(&mut second, 26, -0.003);
    put_f64(&mut second, 34, 0.003);
    put_f64(&mut second, 42, 1.0);
    put_f64(&mut second, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut second, at, 1);
    }
    stream.extend(second);
    stream
}

pub(crate) fn intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

pub(crate) fn charted_intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 1, 20, 21, 22, 23].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);

    let mut chart = record(40, 108);
    chart[2..6].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_f64(&mut chart, 28, 0.000_01);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    put_vec3(&mut chart, 60, [0.0, 0.0, 0.0]);
    put_vec3(&mut chart, 84, [0.01, 0.0, 0.0]);
    stream.extend(chart);

    for (xmt, point) in [(21, [0.0, 0.0, 0.0]), (22, [0.01, 0.0, 0.0])] {
        let mut term = record(41, 34);
        term[2..6].copy_from_slice(&1u32.to_be_bytes());
        put_ref(&mut term, 6, xmt);
        term[8..10].copy_from_slice(b"L?");
        put_vec3(&mut term, 10, point);
        stream.extend(term);
    }

    let mut uv = record(204, 41);
    uv[2..6].copy_from_slice(&4u32.to_be_bytes());
    put_ref(&mut uv, 6, 23);
    uv[8] = 2;
    for (index, value) in [0.0, 0.0, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut uv, 9 + index * 8, value);
    }
    stream.extend(uv);
    stream
}

pub(crate) fn charted_intersection_with_edge_endpoint_witnesses_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let first_fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin record");
    put_ref(&mut stream, first_fin + 8, 13);
    put_ref(&mut stream, first_fin + 10, 13);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point record");
    put_vec3(&mut stream, first_point + 16, [0.0, 0.0, 0.0]);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 13);
    put_ref(&mut second_fin, 6, 5);
    put_ref(&mut second_fin, 8, 7);
    put_ref(&mut second_fin, 10, 7);
    put_ref(&mut second_fin, 12, 14);
    put_ref(&mut second_fin, 14, 1);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 12);
    second_fin[22] = b'+';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 14);
    put_ref(&mut second_vertex, 16, 15);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 15);
    put_vec3(&mut second_point, 16, [0.01, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

pub(crate) fn charted_intersection_without_uv_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 29, 1);
    stream
}

pub(crate) fn charted_intersection_with_approximated_term_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let end = stream
        .windows(8)
        .position(|window| window == [0, 41, 0, 0, 0, 1, 0, 22])
        .expect("end term record");
    put_f64(&mut stream, end + 10, 0.010_005);
    stream
}

pub(crate) fn ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, point) in [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0]].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, point);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, [2.0, 5.0][index]);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

pub(crate) fn two_support_ext11_charted_intersection_curve_stream(ambiguous: bool) -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 29, 1);

    let second_plane = stream
        .windows(4)
        .position(|window| window == [0, 50, 0, 13])
        .expect("second plane");
    if !ambiguous {
        put_vec3(&mut stream, second_plane + 67, [0.0, 0.0, 1.0]);
    }

    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, x) in [0.0, 0.01].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, [x, 0.0, 0.0]);
        let second = if ambiguous { [x, 0.0] } else { [0.0, x] };
        put_f64(&mut entries, at + 24, x);
        put_f64(&mut entries, at + 32, second[0]);
        put_f64(&mut entries, at + 40, 0.0);
        put_f64(&mut entries, at + 48, second[1]);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, x);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

pub(crate) fn partial_ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_ext11_charted_intersection_curve_stream(false);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    for index in 0..2 {
        put_f64(
            &mut stream,
            chart + 60 + index * 88 + 32,
            -31_415_800_000_000.0,
        );
    }
    stream
}

pub(crate) fn two_support_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);

    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    stream[uv + 2..uv + 6].copy_from_slice(&8u32.to_be_bytes());
    stream[uv + 8] = 4;
    let mut values = vec![0u8; 8 * 8];
    for (index, value) in [0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.01, 0.0]
        .into_iter()
        .enumerate()
    {
        put_f64(&mut values, index * 8, value);
    }
    stream.splice(uv + 9..uv + 41, values);

    let mut second_plane = record(50, 91);
    put_ref(&mut second_plane, 2, 13);
    second_plane[18] = b'+';
    put_vec3(&mut second_plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut second_plane, 43, [0.0, 1.0, 0.0]);
    put_vec3(&mut second_plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(second_plane);
    stream
}

pub(crate) fn blend_bound_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 14);

    let mut bridge = record(59, 24);
    put_ref(&mut bridge, 2, 14);
    bridge[4..8].copy_from_slice(&9u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut bridge, at, 1);
    }
    bridge[18] = b'+';
    put_ref(&mut bridge, 19, 0);
    put_ref(&mut bridge, 21, 13);
    stream.extend(bridge);
    stream
}

pub(crate) fn inline_descriptor_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    let mut inline_uv = b"values\x00\x00\x00\x02\x01\x66\x01".to_vec();
    inline_uv.extend_from_slice(&4u32.to_be_bytes());
    inline_uv.extend_from_slice(&23u16.to_be_bytes());
    inline_uv.push(2);
    for value in [0.0_f64, 0.0, 0.01, 0.0] {
        inline_uv.extend_from_slice(&value.to_be_bytes());
    }
    stream.splice(uv..uv + 41, inline_uv);

    for (xmt, point) in [(22u16, [0.01_f64, 0.0, 0.0]), (21, [0.0, 0.0, 0.0])] {
        let marker = [0, 41, 0, 0, 0, 1, (xmt >> 8) as u8, xmt as u8];
        let term = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("term record");
        let mut inline = b"term_use\x00\x00\x00\x01\x01\x63\x43\x5a".to_vec();
        inline.extend_from_slice(&1u32.to_be_bytes());
        inline.extend_from_slice(&xmt.to_be_bytes());
        inline.extend_from_slice(b"L?");
        for coordinate in point {
            inline.extend_from_slice(&coordinate.to_be_bytes());
        }
        stream.splice(term..term + 34, inline);
    }
    stream
}

pub(crate) fn deltas_intersection_curve_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let subtype = stream
        .windows(b"(partition)".len())
        .position(|window| window == b"(partition)")
        .expect("partition subtype");
    stream.splice(
        subtype..subtype + b"(partition)".len(),
        b"(deltas)".iter().copied(),
    );
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    stream.extend_from_slice(b"intersection_data");
    stream.push(0x5a);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream
}

/// Shared `PS`-signatured deltas-stream transmit preamble used by the deltas
/// fixture builders.
pub(crate) const DELTAS_PREAMBLE: &[u8] =
    b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00";

/// Append `count` deltas topology references, each the placeholder index `1`
/// followed by a set status byte, matching the deltas record framing.
pub(crate) fn push_reference_run(record: &mut Vec<u8>, count: usize) {
    for _ in 0..count {
        record.extend_from_slice(&1u16.to_be_bytes());
        record.push(1);
    }
}

pub(crate) fn status_framed_deltas_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    let mut face = Vec::new();
    face.extend_from_slice(&14u16.to_be_bytes());
    face.extend_from_slice(&100u16.to_be_bytes());
    face.extend_from_slice(&7u32.to_be_bytes());
    let push_ref = |record: &mut Vec<u8>, reference: u16| {
        record.extend_from_slice(&reference.to_be_bytes());
        record.push(1);
    };
    push_ref(&mut face, 1);
    face.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
    push_reference_run(&mut face, 5);
    face.push(b'+');
    push_reference_run(&mut face, 5);
    stream.extend(face);
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&50_000u16.to_be_bytes());
    stream.extend_from_slice(&[0, 1]);
    stream
}

pub(crate) fn variable_status_framed_deltas_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&(-100i16).to_be_bytes());
    stream.extend_from_slice(&0u16.to_be_bytes());
    stream.extend_from_slice(&8u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&101u16.to_be_bytes());
    stream.extend_from_slice(&9u32.to_be_bytes());
    for reference in [1u16, 2] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn status_framed_deltas_point_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&29u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&900u32.to_be_bytes());
    for reference in [1u16; 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for value in [0.0125f64, -0.002, 0.004] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

pub(crate) fn status_framed_deltas_intersection_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&38u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [6u16, 7, 20, 21, 22, 23] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_point_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend(status_framed_deltas_point_stream());
    stream
}

pub(crate) fn deltas_edge_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&8u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_9f64.to_be_bytes());
    for reference in [7u16, 1, 1, 9, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_face_vertex_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&14u16.to_be_bytes());
    stream.extend_from_slice(&4u16.to_be_bytes());
    stream.extend_from_slice(&902u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_8f64.to_be_bytes());
    for reference in [1u16, 1, 5, 3, 6] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    push_reference_run(&mut stream, 5);

    stream.extend_from_slice(&18u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&903u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 11] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&0.000_7f64.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream
}

pub(crate) fn deltas_loop_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&5u16.to_be_bytes());
    stream.extend_from_slice(&904u32.to_be_bytes());
    for reference in [1u16, 7, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_shell_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&3u16.to_be_bytes());
    stream.extend_from_slice(&905u32.to_be_bytes());
    for reference in [1u16, 2, 1, 4, 1, 1, 12, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn deltas_fin_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&7u16.to_be_bytes());
    for reference in [1u16, 5, 7, 7, 10, 1, 8, 9, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    stream
}

/// Build a deltas analytic-surface partition record: the shared transmit
/// preamble, a `type`/`xmt`/`node_id` header, a five-reference run, the `+`
/// status marker, and the shape's big-endian `f64` payload values.
pub(crate) fn deltas_analytic_partition_stream(
    type_code: u16,
    xmt: u16,
    node_id: u32,
    values: &[f64],
) -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&type_code.to_be_bytes());
    stream.extend_from_slice(&xmt.to_be_bytes());
    stream.extend_from_slice(&node_id.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for value in values {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

pub(crate) fn deltas_line_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(30, 9, 906, &[0.004, 0.005, 0.006, 0.0, 1.0, 0.0])
}

pub(crate) fn deltas_plane_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        50,
        6,
        907,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
}

pub(crate) fn deltas_offset_surface_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&60u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&907u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    stream.extend_from_slice(b"V");
    stream.push(1);
    stream.extend_from_slice(&6u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.004_5f64.to_be_bytes());
    stream
}

pub(crate) fn status_frame_compact_references(
    mut record: Vec<u8>,
    reference_offsets: &[usize],
) -> Vec<u8> {
    for &offset in reference_offsets.iter().rev() {
        record.insert(offset + 2, 1);
    }
    record
}

pub(crate) fn deltas_stream_with_record(record: Vec<u8>) -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend(record);
    stream
}

pub(crate) fn deltas_blend_surface_partition_stream() -> Vec<u8> {
    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[4..8].copy_from_slice(&908u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut blend, at, 1);
    }
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.004);
    put_f64(&mut blend, 34, 0.004);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    deltas_stream_with_record(status_frame_compact_references(
        blend,
        &[8, 10, 12, 14, 16, 20, 22, 24, 58, 60, 62, 64],
    ))
}

pub(crate) fn deltas_trimmed_curve_partition_stream() -> Vec<u8> {
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[4..8].copy_from_slice(&909u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut trim, at, 1);
    }
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_3);
    put_f64(&mut trim, 77, 0.000_7);
    deltas_stream_with_record(status_frame_compact_references(
        trim,
        &[8, 10, 12, 14, 16, 19],
    ))
}

pub(crate) fn deltas_surface_curve_partition_stream() -> Vec<u8> {
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[4..8].copy_from_slice(&910u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut surface_curve, at, 1);
    }
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_02);
    deltas_stream_with_record(status_frame_compact_references(
        surface_curve,
        &[8, 10, 12, 14, 16, 19, 21, 23],
    ))
}

/// Point the single partition face record at geometry reference `reference`.
pub(crate) fn link_partition_face(stream: &mut Vec<u8>, reference: u16) {
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(stream, face + 26, reference);
}

/// Point both the edge and fin topology records at geometry reference
/// `reference`.
pub(crate) fn link_partition_edge_and_fin(stream: &mut Vec<u8>, reference: u16) {
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(stream, record + field, reference);
    }
}

pub(crate) fn circle_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_edge_and_fin(&mut stream, 12);
    let mut circle = record(31, 99);
    put_ref(&mut circle, 2, 12);
    circle[18] = b'+';
    put_vec3(&mut circle, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut circle, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut circle, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut circle, 91, 0.01);
    stream.extend(circle);
    stream
}

pub(crate) fn deltas_circle_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        31,
        12,
        908,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.025],
    )
}

pub(crate) fn ellipse_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_edge_and_fin(&mut stream, 13);
    let mut ellipse = record(32, 107);
    put_ref(&mut ellipse, 2, 13);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.02);
    put_f64(&mut ellipse, 99, 0.01);
    stream.extend(ellipse);
    stream
}

pub(crate) fn deltas_ellipse_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        32,
        13,
        909,
        &[
            0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.03, 0.012,
        ],
    )
}

pub(crate) fn cylinder_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 12);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, 0.01);
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);
    stream.extend(cylinder);
    stream
}

pub(crate) fn deltas_cylinder_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        51,
        12,
        910,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 0.025, 1.0, 0.0, 0.0],
    )
}

pub(crate) fn cone_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 12);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.01);
    put_f64(&mut cone, 75, 0.0);
    put_f64(&mut cone, 83, 1.0);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    stream.extend(cone);
    stream
}

pub(crate) fn deltas_cone_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        52,
        12,
        911,
        &[
            0.001,
            0.002,
            0.003,
            0.0,
            1.0,
            0.0,
            0.025,
            0.5,
            3.0f64.sqrt() / 2.0,
            1.0,
            0.0,
            0.0,
        ],
    )
}

pub(crate) fn sphere_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 12);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.0, 0.0, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [0.0, 0.0, 1.0]);
    put_vec3(&mut sphere, 75, [1.0, 0.0, 0.0]);
    stream.extend(sphere);
    stream
}

pub(crate) fn deltas_sphere_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        53,
        12,
        912,
        &[0.001, 0.002, 0.003, 0.025, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
}

pub(crate) fn torus_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 12);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut torus, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [1.0, 0.0, 0.0]);
    stream.extend(torus);
    stream
}

pub(crate) fn deltas_torus_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        54,
        12,
        913,
        &[
            0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 0.04, 0.015, 1.0, 0.0, 0.0,
        ],
    )
}

pub(crate) fn bspline_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
    let mut surface = record(124, 23);
    put_ref(&mut surface, 2, 10);
    surface[18] = b'+';
    put_ref(&mut surface, 19, 20);
    put_ref(&mut surface, 21, 21);
    s.extend(surface);

    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 21);
    s.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 21);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    s.extend(data);

    for (tag, reference, values) in [(127, 30, vec![2u16, 2]), (127, 31, vec![2, 2])] {
        let mut array = record(tag, 8 + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        put_ref(&mut array, 6, reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 8 + index * 2, value);
        }
        s.extend(array);
    }
    for reference in [32, 33] {
        let mut array = record(128, 8 + 2 * 8);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        put_f64(&mut array, 8, 0.0);
        put_f64(&mut array, 16, 1.0);
        s.extend(array);
    }

    let mut curve = record(134, 23);
    put_ref(&mut curve, 2, 50);
    curve[18] = b'+';
    put_ref(&mut curve, 19, 40);
    put_ref(&mut curve, 21, 41);
    s.extend(curve);
    let mut curve_descriptor = record(136, 27);
    put_ref(&mut curve_descriptor, 2, 40);
    put_ref(&mut curve_descriptor, 4, 1);
    put_ref(&mut curve_descriptor, 8, 2);
    put_ref(&mut curve_descriptor, 10, 3);
    put_ref(&mut curve_descriptor, 14, 2);
    curve_descriptor[16] = 5;
    put_ref(&mut curve_descriptor, 23, 42);
    put_ref(&mut curve_descriptor, 25, 43);
    s.extend(curve_descriptor);
    let mut curve_data = record(135, 15 + 6 * 8);
    put_ref(&mut curve_data, 2, 41);
    curve_data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.0, 0.0].into_iter().enumerate() {
        put_f64(&mut curve_data, 15 + index * 8, value);
    }
    s.extend(curve_data);
    for (tag, reference) in [(127, 42), (128, 43)] {
        let mut array = record(tag, if tag == 127 { 12 } else { 24 });
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        if tag == 127 {
            put_ref(&mut array, 8, 2);
            put_ref(&mut array, 10, 2);
        } else {
            put_f64(&mut array, 8, 0.0);
            put_f64(&mut array, 16, 1.0);
        }
        s.extend(array);
    }
    s
}

pub(crate) fn extended_bspline_surface_stream() -> Vec<u8> {
    let descriptor_ref = 40_000u32;
    let payload_ref = 40_001u32;
    let support_refs = [40_010u32, 40_011, 40_012, 40_013];

    let mut stream = Vec::new();
    let mut wrapper = record(124, 19);
    put_ref(&mut wrapper, 2, 10);
    wrapper[18] = b'+';
    stream.extend(wrapper);
    stream.extend(encoded_xmt(descriptor_ref));
    stream.extend(encoded_xmt(payload_ref));

    let xmt = encoded_xmt(descriptor_ref);
    let shift = xmt.len() - 2;
    let mut descriptor = vec![0u8; 58 + shift];
    descriptor[..2].copy_from_slice(&126u16.to_be_bytes());
    descriptor[2..2 + xmt.len()].copy_from_slice(&xmt);
    put_ref(&mut descriptor, 6 + shift, 1);
    put_ref(&mut descriptor, 8 + shift, 1);
    put_ref(&mut descriptor, 12 + shift, 2);
    put_ref(&mut descriptor, 16 + shift, 2);
    descriptor[18 + shift] = 5;
    descriptor[19 + shift] = 5;
    descriptor[20 + shift..24 + shift].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24 + shift..28 + shift].copy_from_slice(&2u32.to_be_bytes());
    let mut at = 34 + shift;
    for reference in [
        40_009,
        support_refs[0],
        support_refs[1],
        support_refs[2],
        support_refs[3],
    ] {
        let encoded = encoded_xmt(reference);
        descriptor[at..at + encoded.len()].copy_from_slice(&encoded);
        at += encoded.len();
    }
    assert_eq!(at, 54 + shift);
    put_ref(&mut descriptor, 54 + shift, 125);
    stream.extend(descriptor);

    let xmt = encoded_xmt(payload_ref);
    let shift = xmt.len() - 2;
    let first = encoded_xmt(40_020);
    let data_at = 95 + shift + first.len();
    let mut payload = vec![0u8; data_at + 12 * 8];
    payload[..2].copy_from_slice(&125u16.to_be_bytes());
    payload[2..2 + xmt.len()].copy_from_slice(&xmt);
    payload[90 + shift] = b'+';
    payload[91 + shift..95 + shift].copy_from_slice(&12u32.to_be_bytes());
    payload[95 + shift..data_at].copy_from_slice(&first);
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut payload, data_at + index * 8, value);
    }
    stream.extend(payload);

    for (tag, reference, values) in [
        (127, support_refs[0], vec![2u16, 2]),
        (127, support_refs[1], vec![2, 2]),
    ] {
        let reference = encoded_xmt(reference);
        let mut array = record(tag, 6 + reference.len() + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 6 + reference.len() + index * 2, value);
        }
        stream.extend(array);
    }
    for reference in [support_refs[2], support_refs[3]] {
        let reference = encoded_xmt(reference);
        let mut array = record(128, 6 + reference.len() + 16);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        put_f64(&mut array, 6 + reference.len(), 0.0);
        put_f64(&mut array, 14 + reference.len(), 1.0);
        stream.extend(array);
    }
    stream
}

pub(crate) fn bspline_surface_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 60);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 61);
    stream.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 61);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.03, 0.0, 0.015, 0.0, 0.0, 0.015, 0.03, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    stream.extend(data);
    stream
}

pub(crate) fn deltas_bspline_surface_wrapper_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&124u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&914u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for reference in [60u16, 61] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn bspline_curve_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 70);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 42);
    put_ref(&mut descriptor, 25, 43);
    stream.extend(descriptor);

    let mut data = record(135, 15 + 6 * 8);
    put_ref(&mut data, 2, 71);
    data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut data, 15 + index * 8, value);
    }
    stream.extend(data);
    stream
}

pub(crate) fn deltas_bspline_curve_wrapper_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&134u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&915u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for reference in [70u16, 71] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

pub(crate) fn trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 12);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_25);
    put_f64(&mut trim, 77, 0.000_75);
    // The closed edge's single vertex sits at the trim range's midpoint on the
    // basis line so both trimmed endpoints fall inside the edge's stored
    // 0.3 mm tolerance; the point record is the topology stream's last
    // 40 bytes, before the trim record is appended.
    let point_vec = stream.len() - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.0, 0.0]);
    stream.extend(trim);
    stream
}

pub(crate) fn mismatched_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let point_vec = stream.len() - 85 - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.01, 0.0]);
    stream
}

pub(crate) fn partnered_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, 0.000_75);
    put_f64(&mut stream, trim + 77, 0.000_25);
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("first face");
    put_ref(&mut stream, face + 18, 20);
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin");
    put_ref(&mut stream, fin + 14, 22);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point");
    put_vec3(&mut stream, first_point + 16, [0.000_25, 0.0, 0.0]);

    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 4);
    put_ref(&mut second_face, 22, 21);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let mut second_loop = record(15, 16);
    put_ref(&mut second_loop, 2, 21);
    put_ref(&mut second_loop, 10, 22);
    put_ref(&mut second_loop, 12, 20);
    put_ref(&mut second_loop, 14, 1);
    stream.extend(second_loop);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 22);
    put_ref(&mut second_fin, 6, 21);
    put_ref(&mut second_fin, 8, 22);
    put_ref(&mut second_fin, 10, 22);
    put_ref(&mut second_fin, 12, 23);
    put_ref(&mut second_fin, 14, 7);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 1);
    second_fin[22] = b'-';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 23);
    put_ref(&mut second_vertex, 16, 24);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 24);
    put_vec3(&mut second_point, 16, [0.000_75, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

pub(crate) fn forward_trimmed_curve_chain_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("first trimmed curve");
    put_ref(&mut stream, first + 19, 20);

    let mut second = record(133, 85);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    put_ref(&mut second, 19, 9);
    put_f64(&mut second, 69, 0.000_25);
    put_f64(&mut second, 77, 0.000_75);
    stream.extend(second);
    stream
}

pub(crate) fn topology_with_extended_edge_curve_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 24..edge + 26].copy_from_slice(&(-9i16).to_be_bytes());
    stream.splice(edge + 26..edge + 26, [0, 0]);
    stream
}

pub(crate) fn topology_with_extended_face_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    stream.splice(face + 8..face + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

pub(crate) fn topology_with_extended_edge_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream.splice(edge + 8..edge + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

pub(crate) fn topology_with_extended_internal_topology_references() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(13, 3, 8), (15, 5, 8), (17, 7, 4), (18, 10, 8), (29, 11, 8)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        stream.splice(
            record + offset..record + offset + 2,
            [0xff, 0xff, 0x00, 0x00],
        );
    }
    stream
}

pub(crate) fn topology_with_fully_extended_geometry_headers() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt) in [(50, 6), (30, 9)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        for index in 0..5 {
            let at = record + 8 + index * 4;
            stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
        }
    }
    stream
}

pub(crate) fn topology_with_escaped_geometry_envelopes() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for marker in [[0, 50, 0, 6], [0, 30, 0, 9]] {
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        stream.insert(record + 2, 0xff);
    }
    stream
}

pub(crate) fn offset_surface_with_fully_extended_common_header() -> Vec<u8> {
    let mut stream = offset_surface_topology_partition_stream();
    let record = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
    stream
}

pub(crate) fn fully_extend_common_header(stream: &mut Vec<u8>, marker: [u8; 4]) {
    let record = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("compact geometry record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
}

pub(crate) fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    // Level 1 emits the `78 01` zlib header NX/Parasolid streams use.
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

pub(crate) fn zlib_compress_at_level(raw: &[u8], level: u32) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(level));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

/// Assemble a synthetic single-part `.prt`: the SPLMSSTR header, a HEADER
/// directory with one `/Root/UG_PART/UG_PART` file entry, and a zlib-compressed
/// Parasolid partition stream.
pub(crate) fn single_part_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06); // version tag
    f.extend_from_slice(&[0x11, 0x22, 0x33]); // u24 file tag
    f.extend_from_slice(&[0, 0, 0, 0]); // +0x0c constant
    f.push(0x00); // +0x10 constant
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // +0x11 footer offset (0 → no footer)
    f.extend_from_slice(&[0, 0]); // pad to 0x19
    assert_eq!(f.len(), 0x19);

    // HEADER directory: one file entry naming the canonical part stream.
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/UG_PART";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    // 16-byte payload: file_offset then size (both u64 LE) — point at the zlib blob.
    let blob = zlib_compress(&partition_stream());
    // The blob will be appended after the directory; compute its offset now.
    let dir_end = f.len() + 16; // after this entry's payload
    let blob_off = dir_end as u64;
    f.extend_from_slice(&blob_off.to_le_bytes());
    f.extend_from_slice(&(blob.len() as u64).to_le_bytes());
    f.extend_from_slice(&blob);
    f
}

pub(crate) fn prt_with_named_payloads(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let mut spans = Vec::new();
    for (name, _) in entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name.as_bytes());
        spans.push(file.len());
        file.extend_from_slice(&[0; 16]);
    }
    for ((_, payload), span) in entries.iter().zip(spans) {
        let offset = file.len();
        file.extend_from_slice(payload);
        file[span..span + 8].copy_from_slice(&(offset as u64).to_le_bytes());
        file[span + 8..span + 16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    }
    file
}

pub(crate) fn prt_with_arrangements() -> Vec<u8> {
    prt_with_arrangement_attribute(Some("Model"))
}

pub(crate) fn prt_with_arrangement_attribute(active_name: Option<&str>) -> Vec<u8> {
    let mut arrangements = br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Exploded"/></Arrangements>"#.to_vec();
    arrangements.push(0);
    let mut attributes = match active_name {
        Some(active_name) => format!(
            r#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="{active_name}" version="3" type="StringAttributeType"/></UgAttributes>"#,
        )
        .into_bytes(),
        None => br#"<UgAttributes version="4"></UgAttributes>"#.to_vec(),
    };
    attributes.push(0);
    prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        ("/Root/part/arrangements", arrangements),
        ("/Root/part/attrs", attributes),
    ])
}

pub(crate) fn topology_part_prt() -> Vec<u8> {
    prt_with_partition(&topology_partition_stream())
}

pub(crate) fn topology_with_missing_tolerances() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(14, 4, 10), (16, 8, 10), (18, 10, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_f64(&mut stream, record + offset, -31_415_800_000_000.0);
    }
    stream
}

pub(crate) fn prt_with_partition(stream: &[u8]) -> Vec<u8> {
    let mut f = single_part_prt();
    let compressed = zlib_compress(stream);
    let entry = container::scan_bytes(f.clone()).unwrap().entries.remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, f.len());
    f.truncate(offset as usize);
    f.extend_from_slice(&compressed);
    let size_at = offset as usize - 8;
    f[size_at..size_at + 8].copy_from_slice(&(compressed.len() as u64).to_le_bytes());
    f
}

pub(crate) fn prt_with_streams(streams: &[&[u8]]) -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let payload = streams
        .iter()
        .flat_map(|stream| zlib_compress(stream))
        .collect::<Vec<_>>();
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

pub(crate) fn prt_with_indexed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = indexed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

pub(crate) fn prt_with_size_framed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = size_framed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

pub(crate) fn large_xmt_headers(stream: &[u8]) -> Vec<u8> {
    let marker = b"SCH_TEST_1_9999\x00";
    let start = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    let lengths = [24, 24, 39, 16, 23, 32, 91, 67, 28, 16, 40];
    let mut out = stream[..start].to_vec();
    let mut pos = start;
    for len in lengths {
        let record = &stream[pos..pos + len];
        let xmt = u16::from_be_bytes([record[2], record[3]]);
        out.extend_from_slice(&record[..2]);
        out.extend_from_slice(&(-(i16::try_from(xmt).unwrap())).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&record[4..]);
        pos += len;
    }
    out
}

/// A synthetic assembly `.prt`: SPLMSSTR header, an `ExternalReferences` file
/// entry, and no embedded Parasolid stream.
pub(crate) fn assembly_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0, 0, 0]);
    f.extend_from_slice(&[0, 0, 0, 0]);
    f.push(0x00);
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    f.extend_from_slice(&[0u8; 16]); // opaque directory payload
    f
}

pub(crate) fn assembly_with_external_paths() -> Vec<u8> {
    let payload = b"EXTREFSTREAM\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend_from_slice(payload);
    f
}

pub(crate) fn rmfastload_prt() -> Vec<u8> {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1..=50u32 {
        payload.extend_from_slice(&id.to_le_bytes());
    }
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(6);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/FastLoad/RMFastLoad";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend(payload);
    f
}

pub(crate) fn many_face_partition_stream(node_id_start: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(
        b"PS\x00\x00XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );
    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    body[4..8].copy_from_slice(&(node_id_start + 100).to_be_bytes());
    stream.extend(body);
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    shell[4..8].copy_from_slice(&(node_id_start + 101).to_be_bytes());
    put_ref(&mut shell, 8, 1);
    put_ref(&mut shell, 10, 2);
    put_ref(&mut shell, 12, 1);
    put_ref(&mut shell, 14, 300);
    put_ref(&mut shell, 16, 1);
    put_ref(&mut shell, 18, 1);
    put_ref(&mut shell, 20, 4);
    put_ref(&mut shell, 22, 1);
    stream.extend(shell);
    let mut region = record(19, 16);
    put_ref(&mut region, 2, 4);
    stream.extend(region);
    for index in 0..50u16 {
        let mut face = record(14, 39);
        put_ref(&mut face, 2, 300 + index);
        face[4..8].copy_from_slice(&(node_id_start + u32::from(index)).to_be_bytes());
        put_f64(&mut face, 10, 0.000_1);
        put_ref(&mut face, 18, if index == 49 { 1 } else { 301 + index });
        put_ref(&mut face, 20, if index == 0 { 1 } else { 299 + index });
        put_ref(&mut face, 22, 1);
        put_ref(&mut face, 24, 3);
        put_ref(&mut face, 26, 500 + index);
        face[28] = b'+';
        stream.extend(face);
    }
    for index in 0..50u16 {
        let mut plane = record(50, 91);
        put_ref(&mut plane, 2, 500 + index);
        plane[18] = b'+';
        put_vec3(&mut plane, 19, [f64::from(index) * 0.001, 0.0, 0.0]);
        put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
        stream.extend(plane);
    }
    stream
}

pub(crate) fn prt_with_two_bodies_and_rmfastload() -> Vec<u8> {
    let mut part_payload = zlib_compress(&many_face_partition_stream(1_000));
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

pub(crate) fn prt_with_two_active_bodies_and_rmfastload() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let count_at = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len();
    let ids_at = count_at + 4;
    let tail = file[ids_at + 50 * 4..].to_vec();
    file[count_at..count_at + 4].copy_from_slice(&100u32.to_le_bytes());
    file.truncate(ids_at + 50 * 4);
    for id in 2_000..2_050u32 {
        file.extend_from_slice(&id.to_le_bytes());
    }
    file.extend_from_slice(&tail);
    let directory_size_at = file
        .windows(b"/Root/FastLoad/RMFastLoad".len())
        .position(|window| window == b"/Root/FastLoad/RMFastLoad")
        .expect("RMFastLoad directory")
        + b"/Root/FastLoad/RMFastLoad".len()
        + 8;
    file[directory_size_at..directory_size_at + 8]
        .copy_from_slice(&((marker.len() + 4 + 100 * 4) as u64).to_le_bytes());
    file
}

pub(crate) fn prt_with_missing_active_body_record() -> Vec<u8> {
    let mut active_stream = many_face_partition_stream(1_000);
    let body = active_stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    active_stream[body..body + 24].fill(0xff);
    let mut part_payload = zlib_compress(&active_stream);
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

pub(crate) fn prt_with_weak_rmfastload_overlap() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let payload = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len()
        + 4;
    for index in 0..50u32 {
        let id = if index < 5 {
            1_000 + index
        } else {
            10_000 + index
        };
        let at = payload + index as usize * 4;
        file[at..at + 4].copy_from_slice(&id.to_le_bytes());
    }
    file
}
