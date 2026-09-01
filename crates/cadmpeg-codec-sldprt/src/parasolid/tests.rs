// SPDX-License-Identifier: Apache-2.0
//! Parasolid stream split, header, and mesh-polyline tests.
#![allow(clippy::unwrap_used)]

use std::io::Write as _;

use crate::container;
use crate::test_support::*;
use flate2::{write::ZlibEncoder, Compression};

fn zlib_member(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("write zlib frame");
    encoder.finish().expect("finish zlib frame")
}

fn chained_payload(sections: &[Vec<Vec<u8>>]) -> (Vec<u8>, Vec<usize>) {
    let mut payload = Vec::new();
    let mut offsets = Vec::new();
    for frames in sections {
        let mut section = WRAPPED_MAGIC.to_vec();
        for frame in frames {
            let member = zlib_member(frame);
            section.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            section.extend_from_slice(&(member.len() as u32).to_le_bytes());
            section.extend_from_slice(&member);
        }
        section.extend_from_slice(&[0; 8]);
        offsets.push(payload.len());
        payload.extend_from_slice(&(section.len() as u32).to_le_bytes());
        payload.extend_from_slice(&section);
    }
    (payload, offsets)
}

const WRAPPED_MAGIC: [u8; 16] = [
    0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99,
];

#[test]
fn parasolid_stream_header_is_parsed() {
    let f = synthetic_sldprt();
    let scan = container::scan_bytes(&f);
    let (block, header) = container::select_active_parasolid(&scan).expect("active parasolid");
    assert_eq!(header.schema, "SCH_SW_33103_11000");
    assert!(header.description.contains("partition"));
    assert_eq!(block.family, "parasolid");
    assert!(crate::parasolid::is_body_stream(header));
}

#[test]
fn parasolid_extracts_every_direct_stream_in_block() {
    let mut payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    payload.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &world_point(60, [2.0, 0.0, 0.0]),
    ));
    let streams = crate::parasolid::extract_streams_with_offsets(&payload);
    assert_eq!(streams.len(), 2);
    assert!(streams[0].header.description.contains("partition"));
    assert!(streams[1].header.description.contains("deltas"));
}

#[test]
fn parasolid_reassembles_chained_sections_before_header_parsing() {
    let partition = parasolid_with_body("partition body", "SCH_SW_33103_11000", &vec![0x31; 5000]);
    let deltas = parasolid_with_body("deltas body", "SCH_SW_33103_11000", &vec![0x42; 3000]);
    let partition_split = 7;
    let deltas_split = 19;
    let (payload, offsets) = chained_payload(&[
        vec![
            partition[..partition_split].to_vec(),
            partition[partition_split..].to_vec(),
        ],
        vec![
            deltas[..deltas_split].to_vec(),
            deltas[deltas_split..].to_vec(),
        ],
    ]);

    let streams = crate::parasolid::extract_streams_with_offsets(&payload);
    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0].offset, offsets[0]);
    assert_eq!(streams[0].payload, partition);
    assert_eq!(streams[1].offset, offsets[1]);
    assert_eq!(streams[1].payload, deltas);
}

#[test]
fn parasolid_reassembles_the_degenerate_one_frame_wrapper() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let member = zlib_member(&stream);
    let mut payload = WRAPPED_MAGIC.to_vec();
    payload.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(member.len() as u32).to_le_bytes());
    payload.extend_from_slice(&member);
    payload.extend_from_slice(b"trailer!");

    let streams = crate::parasolid::extract_streams_with_offsets(&payload);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].offset, 0);
    assert_eq!(streams[0].payload, stream);
}

#[test]
fn wrapped_member_requires_the_parasolid_header_at_byte_zero() {
    let mut stream = b"prefix".to_vec();
    stream.extend(parasolid_payload("partition body", "SCH_SW_33103_11000"));
    let member = zlib_member(&stream);
    let mut payload = WRAPPED_MAGIC.to_vec();
    payload.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(member.len() as u32).to_le_bytes());
    payload.extend_from_slice(&member);

    assert!(crate::parasolid::extract_streams_with_offsets(&payload).is_empty());
}

#[test]
fn malformed_chained_continuation_is_not_emitted_as_a_prefix_stream() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let member = zlib_member(&stream);
    let mut section = WRAPPED_MAGIC.to_vec();
    section.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    section.extend_from_slice(&(member.len() as u32).to_le_bytes());
    section.extend_from_slice(&member);
    section.extend_from_slice(b"bad");
    let mut payload = (section.len() as u32).to_le_bytes().to_vec();
    payload.extend_from_slice(&section);

    assert!(crate::parasolid::extract_streams_with_offsets(&payload).is_empty());
}

#[test]
fn parasolid_does_not_split_at_an_unframed_interior_signature() {
    assert!(
        crate::parasolid::extract_streams_with_offsets(b"PS\0\0not-a-stream-header").is_empty()
    );

    let mut first = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    first.extend_from_slice(b"PS\0\0not-a-stream-header");
    let second = parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &world_point(60, [2.0, 0.0, 0.0]),
    );
    let second_offset = first.len();
    first.extend_from_slice(&second);

    let streams = crate::parasolid::extract_streams_with_offsets(&first);
    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0].offset, 0);
    assert_eq!(streams[1].offset, second_offset);
    assert!(streams[0].payload.ends_with(b"PS\0\0not-a-stream-header"));
}

#[test]
fn parasolid_mesh_polyline_decodes_counted_xyz_array() {
    let description = b"boundary_polyline mesh";
    let schema = b"SCH_3201255_32001_13006";
    let mut stream = b"PS\0\0".to_vec();
    stream.extend((description.len() as u16).to_be_bytes());
    stream.extend(description);
    stream.push(schema.len() as u8);
    stream.extend(schema);
    stream.extend([0xff, 0xff, 0xff, 0xff, 0x00, 0x22]);
    stream.extend(6u32.to_be_bytes());
    stream.extend([0x00, 0x22]);
    for value in [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
        stream.extend(value.to_be_bytes());
    }
    let header = crate::parasolid::stream_header(&stream).unwrap();
    assert_eq!(
        crate::parasolid::mesh_polyline_from_header(&stream, &header),
        Some(vec![
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0),
        ])
    );
}
