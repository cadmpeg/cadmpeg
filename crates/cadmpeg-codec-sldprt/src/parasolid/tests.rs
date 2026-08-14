// SPDX-License-Identifier: Apache-2.0
//! Parasolid stream split, header, and mesh-polyline tests.
#![allow(clippy::unwrap_used)]

use crate::container;
use crate::test_support::*;

#[test]
fn parasolid_stream_header_is_parsed() {
    let f = synthetic_sldprt();
    let scan = container::scan_bytes(&f);
    let (block, header) = container::select_active_parasolid(&scan).expect("active parasolid");
    assert_eq!(header.schema, "SCH_SW_33103_11000");
    assert!(header.description.contains("partition"));
    assert_eq!(block.family, "parasolid");
    assert!(crate::parasolid::is_body_stream(&header));
}

#[test]
fn parasolid_extracts_every_direct_stream_in_block() {
    let mut payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    payload.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &world_point(60, [2.0, 0.0, 0.0]),
    ));
    let streams = crate::parasolid::extract_streams(&payload);
    assert_eq!(streams.len(), 2);
    assert!(crate::parasolid::stream_header(&streams[0])
        .unwrap()
        .description
        .contains("partition"));
    assert!(crate::parasolid::stream_header(&streams[1])
        .unwrap()
        .description
        .contains("deltas"));
}

#[test]
fn parasolid_does_not_split_at_an_unframed_interior_signature() {
    assert!(crate::parasolid::extract_streams(b"PS\0\0not-a-stream-header").is_empty());

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
    assert_eq!(streams[0].0, 0);
    assert_eq!(streams[1].0, second_offset);
    assert!(streams[0].1.ends_with(b"PS\0\0not-a-stream-header"));
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
    assert_eq!(
        crate::parasolid::mesh_polyline(&stream),
        Some(vec![
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0),
        ])
    );
}
