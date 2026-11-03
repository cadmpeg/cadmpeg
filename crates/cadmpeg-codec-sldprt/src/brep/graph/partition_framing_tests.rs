// SPDX-License-Identifier: Apache-2.0

//! Slice-level partition and deltas stream framing.

use cadmpeg_core::CodecError;

fn slice_streamed_faces(streams: &[(&str, Vec<u8>)]) -> Result<usize, CodecError> {
    use crate::test_support::parasolid::parasolid_with_body;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let mut framed = Vec::new();
    for (description, body) in streams {
        framed.push(parasolid_with_body(description, "SCH_SW_33103_11000", body));
    }
    let mut headers = Vec::new();
    for stream in &framed {
        headers.push(
            crate::parasolid::stream_header(stream)
                .expect("synthetic header parses")
                .expect("synthetic header present"),
        );
    }
    let bodies: Vec<(&[u8], &crate::parasolid::StreamHeader)> = framed
        .iter()
        .zip(&headers)
        .map(|(stream, header)| (stream.as_slice(), header))
        .collect();
    super::decode_bodies(&ctx, &bodies, &cadmpeg_ir::stream_name!("slice"))
        .map(|brep| brep.faces.len())
}

fn drain_point(body: &mut Vec<u8>) {
    use crate::test_support::parasolid::world_point;
    let missing = world_point(60, [0.0, 0.0, 0.0]);
    let at = body
        .windows(missing.len())
        .position(|window| window == missing.as_slice())
        .expect("partition point record");
    body.drain(at..at + missing.len());
}

#[test]
fn partition_and_deltas_streams_merge_at_slice_level() {
    use crate::test_support::parasolid::{triangle_body, tripled_world_point};
    let full = triangle_body();
    assert_eq!(
        slice_streamed_faces(&[("partition body", full.clone())]).expect("partition decodes"),
        1
    );
    let mut partial = full;
    drain_point(&mut partial);
    assert_eq!(
        slice_streamed_faces(&[("partition body", partial.clone())]).expect("partial decodes"),
        0
    );
    let supplied = tripled_world_point(60, [2.0, 0.0, 0.0]);
    assert_eq!(
        slice_streamed_faces(&[("partition body", partial), ("deltas body", supplied)])
            .expect("merged streams decode"),
        1
    );
}

#[test]
fn nonfinite_partition_point_drops_the_face() {
    use crate::test_support::parasolid::{triangle_body, world_point};
    let mut partial = triangle_body();
    drain_point(&mut partial);
    partial.extend(world_point(60, [f64::NAN, 0.0, 0.0]));
    assert_eq!(
        slice_streamed_faces(&[("partition body", partial)]).expect("rejected point decodes"),
        0
    );
}

#[test]
fn truncated_stream_payload_is_not_a_body() {
    use crate::test_support::parasolid::{parasolid_payload, parasolid_with_body, triangle_body};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    let header = crate::parasolid::stream_header(&stream)
        .expect("synthetic header parses")
        .expect("synthetic header present");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let name = &cadmpeg_ir::stream_name!("truncated-payload");
    let cut = header.body_offset - 1;
    assert!(super::decode_bodies(&ctx, &[(&stream[..cut], &header)], name).is_err());
    let header_only = super::decode_bodies(&ctx, &[(&stream[..header.body_offset], &header)], name)
        .expect("header-only framing decodes");
    assert!(header_only.faces.is_empty());
    let empty = parasolid_payload("partition body", "SCH_SW_33103_11000");
    let empty_header = crate::parasolid::stream_header(&empty)
        .expect("empty header parses")
        .expect("empty header present");
    let decoded = super::decode_bodies(&ctx, &[(&empty, &empty_header)], name)
        .expect("framed empty body decodes");
    assert!(decoded.faces.is_empty());
}
