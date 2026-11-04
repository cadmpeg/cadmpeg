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

/// A Deltas PLANE: index, node ID, five status-paired pointers, sense, and
/// its origin, normal, and reference direction.
fn deltas_plane(first: bool, index: u16, origin_x: f64) -> Vec<u8> {
    let mut bytes = vec![0, 50];
    if first {
        bytes.push(255);
    }
    bytes.extend(index.to_be_bytes());
    bytes.extend(7u32.to_be_bytes());
    for _ in 0..5 {
        bytes.extend(1u16.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(b'+');
    for value in [origin_x, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0] {
        bytes.extend(value.to_be_bytes());
    }
    bytes
}

#[test]
fn a_conflict_across_complete_deltas_streams_holds_when_the_site_scans() {
    use crate::test_support::parasolid::{parasolid_with_body, plane_carrier, triangle_body};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_parasolid::test_support::test_pmarks;
    let mut partial = triangle_body();
    let plane = plane_carrier(100, [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let at = partial
        .windows(plane.len())
        .position(|window| window == plane.as_slice())
        .expect("partition plane record");
    partial.drain(at..at + plane.len());
    let partition = parasolid_with_body("partition body", "SCH_SW_33103_11000", &partial);
    let complete = |origin_x| {
        let mut bytes = test_pmarks::prefix(1);
        bytes.extend(deltas_plane(true, 100, origin_x));
        test_pmarks::complete(bytes)
    };
    let incomplete = parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &deltas_plane(false, 200, 0.0),
    );
    let streams = [partition, complete(0.0), complete(0.001), incomplete];
    let headers: Vec<_> = streams
        .iter()
        .map(|stream| crate::parasolid::stream_header(stream).unwrap().unwrap())
        .collect();
    let bodies: Vec<(&[u8], &crate::parasolid::StreamHeader)> = streams
        .iter()
        .zip(&headers)
        .map(|(stream, header)| (stream.as_slice(), header))
        .collect();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let name = &cadmpeg_ir::stream_name!("mixed");
    let plane = |bodies: &[(&[u8], &crate::parasolid::StreamHeader)]| {
        super::decode_bodies(&ctx, bodies, name)
            .unwrap()
            .surfaces
            .iter()
            .any(|surface| {
                matches!(
                    surface.geometry,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(_)
                    )
                )
            })
    };
    assert!(!plane(&bodies[..1]));
    assert!(plane(&[bodies[0], bodies[1], bodies[3]]));
    assert!(!plane(&bodies));
}
