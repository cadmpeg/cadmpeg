// SPDX-License-Identifier: Apache-2.0
use super::decoded_object_names;
use crate::test_support::history::sldprt_with_body_and_resolved_features;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn semantic_writer_splices_two_renames_in_one_lane_at_the_offsets_the_first_moves() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(
        lane.names
            .iter()
            .map(|name| (name.offset, name.value.as_str()))
            .collect::<Vec<_>>(),
        vec![(54, "Sketch1"), (74, "Boss-Extrude1"), (124, "D1")]
    );
    let scalars = lane.scalars.len();

    // "RenamedSketchLong1" is eleven UTF-16 units longer than "Sketch1", so
    // every later name moves twenty-two bytes.
    let renames = [
        crate::history::write::features::FeatureInputRename {
            lane: lane.id.clone(),
            name_index: 0,
            value: cadmpeg_core::nonblank_literal!("RenamedSketchLong1"),
        },
        crate::history::write::features::FeatureInputRename {
            lane: lane.id.clone(),
            name_index: 2,
            value: cadmpeg_core::nonblank_literal!("Dia"),
        },
    ];

    let payload =
        crate::writer::resolved_feature_payload(lane, &native.feature_histories, &renames).unwrap();

    let written = decoded_object_names(&payload, &lane.id);
    assert_eq!(
        written
            .iter()
            .map(|name| (name.offset, name.value.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (54, "RenamedSketchLong1"),
            (96, "Boss-Extrude1"),
            (146, "Dia")
        ]
    );
    assert_eq!(
        crate::resolved_features::scalars::named_scalars_charged(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &lane.id,
            &written
        )
        .unwrap()
        .len(),
        scalars
    );
}
