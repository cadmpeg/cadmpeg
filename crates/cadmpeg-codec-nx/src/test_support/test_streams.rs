// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

use super::*;

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
    put_ref(&mut pt, 2, 2);
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]); // 62.5, 0, 12.7 mm
    s.extend_from_slice(&pt);

    // PLANE (type 50): origin +19, normal +43, x_axis +67.
    let mut pl = record(0x32, 91);
    put_ref(&mut pl, 2, 3);
    pl[18] = b'+';
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]); // 76.2 mm
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    // CYLINDER (type 51): origin +19, axis +43, radius +67, x_axis +75.
    let mut cy = record(0x33, 99);
    put_ref(&mut cy, 2, 4);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05); // 4.05 mm
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    // LINE (type 30): point +19, direction +43.
    let mut ln = record(0x1e, 67);
    put_ref(&mut ln, 2, 5);
    ln[18] = b'+';
    put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&ln);

    s
}

/// Partition stream carrying one complete variable-width Parasolid GROUP record.
pub(crate) fn parasolid_group_partition_stream() -> Vec<u8> {
    let mut stream = partition_stream();
    stream.extend_from_slice(&90u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(4);
    stream.extend_from_slice(&8u16.to_be_bytes());
    stream.push(0);
    stream
}

/// Raw bytes for an `ExternalReferences` container entry: an `EXTREFSTREAM`
/// index over one empty record and one four-slot handle-set record, followed by
/// an end-anchored four-string table. Decoding walks the record index, string
/// table, empty-record form, handle-set slots, and the handle/tagged tail,
/// populating every `external_reference*` arena.
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
/// `display_jt_compressed_elements`.
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
/// partition, range-LOD, tri-strip shape, and geometric-transform node.
/// Decoding populates `display_jt_base_node_data`, `_group_node_data`,
/// `_instance_nodes`, `_partition_nodes`, `_range_lod_nodes`,
/// `_tri_strip_shape_nodes`, and `_geometric_transform_attributes`.
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
/// entity, `00 52`/`00 53` counted value records, `00 54` string record). The
/// `00 51` entity's references resolve to the value and string records, and its
/// definition reference selects the class declaration, so the join arenas
/// (`parasolid_entity_51_numeric_uses`, `parasolid_entity_51_string_uses`,
/// `parasolid_attribute_class_uses`) are populated as well.
pub(crate) fn parasolid_entity_records_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    // `00 4f` attribute identifier with identity xmt 201, followed by its
    // `00 50` definition record with identity xmt 202 and one field.
    s.extend_from_slice(&[0x00, 0x4f]);
    s.extend_from_slice(&10u32.to_be_bytes()); // name length
    s.extend_from_slice(&201u16.to_be_bytes()); // class identity xmt
    s.extend_from_slice(b"ATTR_CLASS");
    s.extend_from_slice(&[0x00, 0x50]); // field-record tag
    s.extend_from_slice(&1u32.to_be_bytes()); // field count
    s.extend_from_slice(&202u16.to_be_bytes()); // field-record xmt
    s.extend_from_slice(&1u16.to_be_bytes()); // null next-definition reference
    s.extend_from_slice(&201u16.to_be_bytes()); // identifier reference
    s.extend_from_slice(&9000u32.to_be_bytes()); // type id
    s.extend_from_slice(&[0; 8]); // event actions
    s.extend_from_slice(&1u16.to_be_bytes()); // null field-name-list reference
    s.extend_from_slice(&[0; 16]); // legal-owner flags
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
    // xmt 50, sequence 2, definition xmt 202. Its references resolve to the
    // string (100), integer (101), and double (102) records above.
    s.extend_from_slice(&[0x00, 0x51]);
    s.extend_from_slice(&1u32.to_be_bytes()); // flags
    s.extend_from_slice(&50u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&2u32.to_be_bytes()); // sequence
    s.extend_from_slice(&202u16.to_be_bytes()); // definition xmt
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
    if !ambiguous {
        let uv = stream
            .windows(8)
            .position(|window| window == [0, 204, 0, 0, 0, 8, 0, 23])
            .expect("UV record");
        put_f64(&mut stream, uv + 9 + 6 * 8, 0.0);
        put_f64(&mut stream, uv + 9 + 7 * 8, 0.01);
    }
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

/// Wrap a partition topology and its ext11 intersection auxiliaries as a paired
/// partition/deltas stream set.
pub(crate) fn prt_with_ext11_intersection(partition: &[u8], ext11: &[u8]) -> Vec<u8> {
    let chart = crate::intersection::chart_source_records(
        ext11,
        crate::intersection::ChartPointLayout::Ext11,
    )
    .into_iter()
    .next()
    .expect("ext11 chart record");
    let (_, chart_end) = crate::intersection::chart_source_record_at(
        ext11,
        chart.pos,
        crate::intersection::ChartPointLayout::Ext11,
    )
    .expect("ext11 chart bounds");
    let mut deltas = DELTAS_PREAMBLE.to_vec();
    deltas.extend_from_slice(&ext11[chart.pos..chart_end]);
    for term in crate::intersection::term_use_records(ext11) {
        let (_, end) = crate::intersection::term_use_at(ext11, term.pos).expect("term bounds");
        deltas.extend_from_slice(&ext11[term.pos..end]);
    }
    let support_uv = crate::intersection::support_uv_records(ext11)
        .into_iter()
        .next()
        .expect("ext11 support UV");
    let (_, support_uv_end) =
        crate::intersection::support_uv_record_at(ext11, support_uv.pos).expect("UV bounds");
    deltas.extend_from_slice(&ext11[support_uv.pos..support_uv_end]);
    prt_with_streams(&[partition, &deltas])
}

pub(crate) fn two_support_charted_intersection_curve_stream() -> Vec<u8> {
    two_support_charted_intersection_curve_stream_with_second_plane_axis([1.0, 0.0, 0.0])
}

pub(crate) fn two_support_charted_intersection_curve_stream_with_second_plane_axis(
    second_plane_axis: [f64; 3],
) -> Vec<u8> {
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
    put_vec3(&mut second_plane, 67, second_plane_axis);
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
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(crate::topology::TYPE_38_SCHEMA_HEADER);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    for reference in [6u16, 7] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for reference in [15u16, 14, 13] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(0);
    }
    stream.extend_from_slice(&[0, 1, 1]);

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
