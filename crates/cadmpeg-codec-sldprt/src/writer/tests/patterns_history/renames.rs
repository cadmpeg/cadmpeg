// SPDX-License-Identifier: Apache-2.0
use super::decoded_object_names;
use crate::test_support::history::sldprt_with_body_and_resolved_features;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn feature_input_rename_preserves_a_declared_name_class() {
    let prefix = [b"\xff\xff\x01\x00\x0c\x00".as_slice(), b"moNodeName_c"].concat();
    check_name_class_rename(&prefix);
}

#[test]
fn feature_input_rename_preserves_a_name_class_token() {
    let prefix = [
        b"\xff\xff\x01\x00\x13\x00".as_slice(),
        b"moSketchBlockInst_c",
        &[3, 0x80],
    ]
    .concat();
    check_name_class_rename(&prefix);
}

fn check_name_class_rename(prefix: &[u8]) {
    let mut payload = prefix.to_vec();
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 3]);
    for unit in "Use".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&102_u32.to_le_bytes());
    payload.extend_from_slice(&[0; 4]);
    let lane = crate::resolved_features::write_prepare::generated_native_lane(
        &[], "sldprt:feature-input:resolved-features#0", None, payload,
    )
    .unwrap();
    let rename = crate::history::write::features::FeatureInputRename {
        lane: lane.id.clone(),
        name_index: 0,
        value: cadmpeg_core::nonblank_literal!("Renamed"),
    };
    let output = crate::writer::resolved_feature_payload(
        &lane,
        &[],
        &[rename],
        crate::writer::ClassLayouts::Authored,
        &std::collections::BTreeSet::new(),
        crate::writer::SourceTrust::Trustworthy,
    )
    .unwrap()
    .0;
    assert!(output.starts_with(prefix));
    let names = decoded_object_names(&output, &lane.id);
    assert_eq!(names[0].value, "Renamed");
    assert_eq!(names[0].object_id, lane.names[0].object_id);
}

#[test]
fn feature_input_rename_reports_utf16_capacity() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let rename = crate::history::write::features::FeatureInputRename {
        lane: lane.id.clone(),
        name_index: 0,
        value: cadmpeg_core::text::NonBlankString::new("\u{1f600}".repeat(32_768)).unwrap(),
    };
    let error: cadmpeg_ir::codec::write::EncodeError = crate::writer::resolved_feature_payload(
        lane,
        &native.feature_histories,
        &[rename],
        crate::writer::ClassLayouts::Source,
        &std::collections::BTreeSet::new(),
        crate::writer::SourceTrust::Trustworthy,
    )
    .unwrap_err()
    .into();
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 65_535, value: 65_536
    } if field.field == "feature_input.name_length")
    );
}

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

    let payload = crate::writer::resolved_feature_payload(
        lane,
        &native.feature_histories,
        &renames,
        crate::writer::ClassLayouts::Source,
        &std::collections::BTreeSet::new(),
        crate::writer::SourceTrust::Trustworthy,
    )
    .unwrap()
    .0;

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
        crate::resolved_features::scalars::named_scalars(&payload, &lane.id, &written)
            .unwrap()
            .len(),
        scalars
    );
}
