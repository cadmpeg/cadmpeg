// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

use super::*;

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

/// A single body-extraction operation whose primary body field resolves to one
/// feature-history-local offset-store block.
pub(crate) fn extract_body_feature_history_prt() -> Vec<u8> {
    let input_slots: &'static [u8] = &[1, 0xff, 0xff, 0xff];
    let body_reference = vec![0x01, 0x02, 0x10, 0x01, 0xff];
    let operations = [(input_slots, "EXTRACT_BODY", body_reference)];
    let store_records: [&[u8]; 2] = [b"\0", b"\0"];
    let payload = composed_feature_history_payload(&operations, &store_records);
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

/// A synthetic history whose two body writers select the same exact
/// offset-store block. The source record order is newest first, so the
/// decoder must attach the older operation as the writer dependency of the
/// newer operation without using the integer block suffix alone.
pub(crate) fn offset_store_primary_body_lineage_prt() -> Vec<u8> {
    let input_slots: &'static [u8] = &[1, 0xff, 0xff, 0xff];
    let primary_body = || vec![0x01, 0x02, 0x10, 0x02, 0xff];
    let operations = [
        (input_slots, "BLEND", primary_body()),
        (input_slots, "EXTRUDE", primary_body()),
    ];
    let store_records: [&[u8]; 2] = [b"\0", b"\0"];
    let payload = composed_feature_history_payload(&operations, &store_records);
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

/// A synthetic history whose Boolean target reuses the native body identity
/// written by the preceding operation and is then consumed by a later
/// operation. The Boolean payload has no separate primary-body field, so its
/// target must supply both writer transitions.
pub(crate) fn boolean_target_body_lineage_prt() -> Vec<u8> {
    let input_slots: &'static [u8] = &[0xff, 0xff, 0xff, 0xff];
    let boolean_payload = b"\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x42\x00\x01\x03\x90\x19\x4c\x7f\x00".to_vec();
    let primary_body = vec![0x01, 0x02, 0x10, 0x90, 0x19, 0x42, 0xff];
    let operations = [
        (input_slots, "EXTRUDE", primary_body.clone()),
        (input_slots, "UNITE", boolean_payload),
        (input_slots, "EXTRUDE", primary_body),
    ];
    let store_records: [&[u8]; 0] = [];
    let payload = composed_feature_history_payload(&operations, &store_records);
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

/// Assemble a synthetic single-part `.prt`: the SPLMSSTR header, a HEADER
/// directory with one `/Root/UG_PART/UG_PART` file entry, and a zlib-compressed
/// Parasolid partition stream.
pub(crate) fn single_part_prt() -> Vec<u8> {
    let mut file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream()))]);
    file[9..12].copy_from_slice(&[0x11, 0x22, 0x33]);
    file
}

pub(crate) fn prt_with_named_payloads(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    file.extend_from_slice(
        &u32::try_from(entries.len())
            .expect("synthetic directory count")
            .to_le_bytes(),
    );
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
    let footer_offset = file.len() as u64;
    file[0x11..0x17].copy_from_slice(&footer_offset.to_le_bytes()[..6]);
    file.extend_from_slice(b"FOOTER");
    file.extend_from_slice(&0_u32.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
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
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", zlib_compress(stream))])
}

pub(crate) fn prt_with_streams(streams: &[&[u8]]) -> Vec<u8> {
    let payload = streams
        .iter()
        .flat_map(|stream| zlib_compress(stream))
        .collect::<Vec<_>>();
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

pub(crate) fn prt_with_indexed_om_section() -> Vec<u8> {
    let mut payload = indexed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

pub(crate) fn prt_with_size_framed_om_section() -> Vec<u8> {
    let mut payload = size_framed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
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
    prt_with_named_payloads(&[("/Root/UG_PART/ExternalReferences", Vec::new())])
}

pub(crate) fn assembly_with_external_paths() -> Vec<u8> {
    let payload = b"EXTREFSTREAM\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    prt_with_named_payloads(&[("/Root/UG_PART/ExternalReferences", payload.to_vec())])
}

pub(crate) fn append_rmfastload_table<I>(payload: &mut Vec<u8>, object_ids: I)
where
    I: IntoIterator<Item = u32>,
{
    let object_ids: Vec<_> = object_ids.into_iter().collect();
    payload.extend_from_slice(
        &u32::try_from(object_ids.len())
            .expect("synthetic RMFastLoad table fits")
            .to_le_bytes(),
    );
    for object_id in object_ids {
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    payload.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0");
}

pub(crate) fn rmfastload_prt() -> Vec<u8> {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, 1..=50);
    prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)])
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

/// Assemble two terminal partition images addressed by a self-bounded segment
/// index. The body identity words are deliberately outside the payload so they
/// cannot be mistaken for additional wrapper offsets.
pub(crate) fn prt_with_two_terminal_bodies() -> Vec<u8> {
    let compressed_streams = [
        zlib_compress(&many_face_partition_stream(1_000)),
        zlib_compress(&many_face_partition_stream(2_000)),
    ];
    let index_byte_len = 72usize;
    let first_wrapper_offset = 96usize;
    let second_wrapper_offset = first_wrapper_offset + 8 + compressed_streams[0].len();
    let index_words = [
        0,
        0,
        0,
        1,
        1,
        index_byte_len as u32,
        first_wrapper_offset as u32,
        0,
        0x1000_0001,
        0x1000_0002,
        19,
        0,
        second_wrapper_offset as u32,
        0,
        0x2000_0001,
        0x2000_0002,
        19,
        0,
    ];
    let mut payload = index_words
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(payload.len(), index_byte_len);
    payload.resize(first_wrapper_offset, 0);
    for compressed in compressed_streams {
        payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&compressed);
        if payload.len() < second_wrapper_offset {
            payload.resize(second_wrapper_offset, 0);
        }
    }
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

pub(crate) fn prt_with_two_bodies_and_rmfastload() -> Vec<u8> {
    let mut part_payload = zlib_compress(&many_face_partition_stream(1_000));
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut rm_payload, 1_000..1_050);

    prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part_payload),
        ("/Root/FastLoad/RMFastLoad", rm_payload),
    ])
}

pub(crate) fn prt_with_two_active_bodies_and_rmfastload() -> Vec<u8> {
    let mut part_payload = zlib_compress(&many_face_partition_stream(1_000));
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut rm_payload, (1_000..1_050).chain(2_000..2_050));

    prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part_payload),
        ("/Root/FastLoad/RMFastLoad", rm_payload),
    ])
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
    append_rmfastload_table(&mut rm_payload, 1_000..1_050);

    prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part_payload),
        ("/Root/FastLoad/RMFastLoad", rm_payload),
    ])
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
