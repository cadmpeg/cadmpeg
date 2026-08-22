// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::*;

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

pub(crate) fn segment_body_binding_repeated_link_payload() -> Vec<u8> {
    let mut rows = [
        [7_u32, 9, 11],
        [1, 1, 0],
        [0, 0, 94],
        [150, 19, 0],
        [0, 0, 95],
        [151, 20, 0],
    ];
    let index_byte_len = u32::try_from(rows.len() * std::mem::size_of::<[u32; 3]>())
        .expect("synthetic segment-index length");
    let wrapper_offset = index_byte_len
        + u32::try_from(std::mem::size_of::<[u32; 4]>()).expect("synthetic wrapper padding length");
    rows[1][2] = index_byte_len;
    rows[2][0] = wrapper_offset;
    rows[4][0] = wrapper_offset;
    let mut payload = rows
        .into_iter()
        .flatten()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    payload.resize(usize::try_from(wrapper_offset).unwrap(), 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(
            b"PS\0\0 (partition) SCH_test repeated stream-link payload with more than sixty-four inflated bytes........",
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

pub(crate) fn segment_om_record_area_with_state_counter_map() -> Vec<u8> {
    let mut payload = segment_om_record_area_payload();
    payload.extend_from_slice(&[
        0x05, 0x01, 0x83, 0x20, 0x01, 0x02, 0x4e, 0x05, 0x02, 0x90, 0x12, 0x34, 0x03, 0x04, 0x4e,
    ]);
    let section_start = 32;
    let section_len = u32::from_be_bytes(
        payload[section_start + 8..section_start + 12]
            .try_into()
            .expect("section length field"),
    );
    payload[section_start + 8..section_start + 12]
        .copy_from_slice(&(section_len + 15).to_be_bytes());
    payload
}

pub(crate) fn segment_om_record_area_with_state_groups_and_counter_map() -> Vec<u8> {
    let mut payload = segment_om_record_area_payload();
    let section_start = 32;
    let marker = b"unframed UGS::PayloadText";
    let field = b"\x14m_rollForwardStates\xa0\x12\x8b";
    let field_at = section_start
        + payload[section_start..]
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("registry tail marker");
    let pointer_at = field_at + marker.len();
    payload.splice(field_at..field_at, field.iter().copied());
    let pointer_at = pointer_at + field.len();
    let pointer = u32::from_le_bytes(
        payload[pointer_at..pointer_at + 4]
            .try_into()
            .expect("record-area pointer"),
    );
    payload[pointer_at..pointer_at + 4]
        .copy_from_slice(&(pointer + field.len() as u32).to_le_bytes());
    let message_bytes = [
        0x03, 0x0f, b's', b't', b'a', b't', b'e', b' ', b'w', b'a', b'r', b'n', b'i', b'n', b'g',
        0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, 0x60, 0x6b, 0x01, 0x00,
    ];
    let state_bytes = [
        0x01, 0x03, 0x4a, 0x83, 0xba, 0x01, 0xff, 0x4a, 0x83, 0xb7, 0x02, 0xff, 0x01, 0x01, 0x01,
        0x02, 0x4f, 0xf1, 0x04, 0x2d, 0x83, 0xe1, 0xff, 0xff, 0x01, 0x01, 0x00, 0x01, 0x01, 0x05,
        0x01, 0x83, 0x20, 0x01, 0x02, 0x4e, 0x05, 0x02, 0x90, 0x12, 0x34, 0x03, 0x04, 0x4e,
    ];
    payload.extend(message_bytes);
    payload.extend(state_bytes);
    let section_len = u32::from_be_bytes(
        payload[section_start + 8..section_start + 12]
            .try_into()
            .expect("section length field"),
    );
    payload[section_start + 8..section_start + 12].copy_from_slice(
        &(section_len
            + u32::try_from(field.len() + message_bytes.len() + state_bytes.len())
                .expect("fixture length"))
        .to_be_bytes(),
    );
    payload
}

pub(crate) fn composed_feature_history_payload_with_operation_state_statuses() -> Vec<u8> {
    let mut section =
        composed_feature_history_section(&[(&[1, 0xff, 0xff, 0xff], "SKETCH", vec![0x00])]);
    let state_bytes = [
        0x41, 0x80, 0x20, 0x3f, 0x44, 0x80, 0x21, 0x4b, 0xff, 0x80, 0x22, 0xff, 0x02, 0x01, 0x11,
        0xff, 0x83, 0xad, 0xff, 0x02, 0x11, 0x03, 0x0f, b's', b't', b'a', b't', b'e', b' ', b'w',
        b'a', b'r', b'n', b'i', b'n', b'g', 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, 0x60, 0x6b, 0x01,
        0x00, 0x05, 0x01, 0x83, 0x20, 0x01, 0x02, 0x4e, 0x05, 0x02, 0x90, 0x12, 0x34, 0x03, 0x04,
        0x4e,
    ];
    let section_len = u32::from_be_bytes(section[8..12].try_into().expect("section length field"));
    section[8..12].copy_from_slice(
        &(section_len + u32::try_from(state_bytes.len()).expect("state fixture length"))
            .to_be_bytes(),
    );
    section.extend_from_slice(&state_bytes);

    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&section);

    let mut store = composed_offset_store(&[]);
    let base = payload.len() as u32;
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    for index in 0..2 {
        let at = index_start + index * 4;
        let value = u32::from_le_bytes(store[at..at + 4].try_into().unwrap());
        store[at..at + 4].copy_from_slice(&(value + base).to_le_bytes());
    }
    payload.extend_from_slice(&store);
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

/// Compose a `UG_PART` payload: segment-index header, one feature-history section
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

    // Object indices do not resolve; only the leading reference/header arenas populate.
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
/// array.
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
/// column records carry the point payload.
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
