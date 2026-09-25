// SPDX-License-Identifier: Apache-2.0
//! Parasolid stream split, header, and mesh-polyline tests.
#![allow(clippy::unwrap_used)]

use std::io::Write as _;

use crate::container;
use crate::test_support::container::synthetic_sldprt;
use crate::test_support::parasolid::parasolid_payload;
use crate::test_support::parasolid::parasolid_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::parasolid::world_point;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
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
fn inner_parasolid_frame_refuses_before_expansion_exceeds_per_expand_limit() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let member = zlib_member(&stream);
    let mut payload = WRAPPED_MAGIC.to_vec();
    payload.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(member.len() as u32).to_le_bytes());
    payload.extend_from_slice(&member);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = stream.len() as u64 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected per-expansion resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!(limit.limit, stream.len() as u64 - 1);
    assert_eq!(limit.used, 0);

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let streams = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].payload, stream);
}

#[test]
fn inner_parasolid_frame_retention_refuses_before_exposing_output() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let member = zlib_member(&stream);
    let mut payload = WRAPPED_MAGIC.to_vec();
    payload.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(member.len() as u32).to_le_bytes());
    payload.extend_from_slice(&member);

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = stream.len() as u64 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected frame-retention refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "inflate Parasolid frame");

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let streams = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap();
    assert_eq!(streams[0].payload, stream);
}

#[test]
fn declared_frame_above_local_cap_still_reports_session_expand_limit() {
    let member = zlib_member(b"x");
    let declared = 512_u32 * 1024 * 1024 + 1;
    let mut payload = WRAPPED_MAGIC.to_vec();
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&(member.len() as u32).to_le_bytes());
    payload.extend_from_slice(&member);

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected the session per-expansion refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
    assert!(limit.limit < u64::from(declared));
}

#[test]
fn chained_parasolid_frames_refuse_cumulative_expansion_limit() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let split = stream.len() / 2;
    let (payload, _) = chained_payload(&[vec![stream[..split].to_vec(), stream[split..].to_vec()]]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = stream.len() as u64 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected cumulative expansion refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!(limit.limit, stream.len() as u64 - 1);
    assert!(limit.used > 0);
}

#[test]
fn chained_parasolid_concatenation_refuses_materialized_limit() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let split = stream.len() / 2;
    let (payload, _) = chained_payload(&[vec![stream[..split].to_vec(), stream[split..].to_vec()]]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = stream.len() as u64 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected concatenation refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.limit, stream.len() as u64 - 1);

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let streams = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap();
    assert_eq!(streams[0].payload, stream);
}

#[test]
fn chained_parasolid_concatenation_refuses_retained_limit() {
    let stream = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let split = stream.len() / 2;
    let (payload, _) = chained_payload(&[vec![stream[..split].to_vec(), stream[split..].to_vec()]]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = stream.len() as u64 * 2 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("root");
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx))
        .expect_err("concatenation must be admitted");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain concatenated Parasolid stream"));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).expect("root");
    let streams = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx))
        .expect("service profile admits concatenation");
    assert_eq!(streams[0].payload, stream);
}

#[test]
fn legacy_zlib_candidate_refuses_probe_work_limit() {
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &vec![0x31; 5000]);
    let member = zlib_member(&stream);
    let mut payload = b"abcdefgh".to_vec();
    payload.extend_from_slice(&WRAPPED_MAGIC);
    payload.extend_from_slice(&member);
    assert!(!payload.windows(4).any(|window| window == b"PS\0\0"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = member.len() as u64 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected candidate work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.limit, member.len() as u64 - 1);

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let streams = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].payload, stream);
}

#[test]
fn legacy_zlib_candidate_refuses_expansion_limit() {
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &vec![0x31; 5000]);
    let member = zlib_member(&stream);
    let mut payload = b"abcdefgh".to_vec();
    payload.extend_from_slice(&WRAPPED_MAGIC);
    payload.extend_from_slice(&member);
    assert!(!payload.windows(4).any(|window| window == b"PS\0\0"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = stream.len() as u64 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = crate::parasolid::extract_streams_with_offsets(&payload, Some(&ctx)).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected candidate expansion refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!(limit.limit, stream.len() as u64 - 1);
}

#[test]
fn parasolid_stream_header_is_parsed() {
    let f = synthetic_sldprt();
    let scan = container::scan_bytes(&f);
    let site = container::select_active_parasolid_site(&scan).expect("active parasolid");
    assert_eq!(site.header.schema.value(), "SCH_SW_33103_11000");
    assert!(site.header.description.contains("partition"));
    let container::Section::Block(block) = site.section else {
        panic!("synthetic native block selected as compound stream");
    };
    assert_eq!(block.family, container::PayloadFamily::Parasolid);
    assert!(crate::parasolid::is_body_stream(site.header));
}

#[test]
fn parasolid_extracts_every_direct_stream_in_block() {
    let mut payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    payload.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &world_point(60, [2.0, 0.0, 0.0]),
    ));
    let streams = crate::parasolid::extract_streams_with_offsets(&payload, None).unwrap();
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

    let streams = crate::parasolid::extract_streams_with_offsets(&payload, None).unwrap();
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

    let streams = crate::parasolid::extract_streams_with_offsets(&payload, None).unwrap();
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

    assert!(
        crate::parasolid::extract_streams_with_offsets(&payload, None)
            .unwrap()
            .is_empty()
    );
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

    assert!(
        crate::parasolid::extract_streams_with_offsets(&payload, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn parasolid_does_not_split_at_an_unframed_interior_signature() {
    assert!(
        crate::parasolid::extract_streams_with_offsets(b"PS\0\0not-a-stream-header", None)
            .unwrap()
            .is_empty()
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

    let streams = crate::parasolid::extract_streams_with_offsets(&first, None).unwrap();
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
