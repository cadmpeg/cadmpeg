// SPDX-License-Identifier: Apache-2.0
//! Sketch-marker lane decode, write-back, and native-validation tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_uses_operand_tag_to_disambiguate_marker_kind() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload_with_names(&[0, 1, 2], &["Sketch1", "D1"]);
    for offset in 0..payload.len().saturating_sub(1) {
        if payload[offset..offset + 2] == [0xd6, 0x80] {
            payload[offset..offset + 2].copy_from_slice(&0x837bu16.to_le_bytes());
        }
    }
    let marker = [0xff, 0xff, 0x1f, 0x00, 0x03];
    let first = payload
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("first marker");
    payload[first + 88..first + 92].copy_from_slice(&2u32.to_le_bytes());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lane = &sldprt_native(decoded.ir()).feature_input_lanes[0];
    let operand = &lane.scalars[0].operands[1];
    assert_eq!(operand.entity_index, 2);
    assert_eq!(
        operand.entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id.as_str())
    );
}

#[test]
fn decode_resolves_each_marker_link_by_trailing_local_id() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload_with_names(&[4, 1, 2], &["Sketch1", "D1"]);
    let marker = [0xff, 0xff, 0x1f, 0x00, 0x03];
    let offset = payload
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("first sketch marker");
    payload[offset + 64..offset + 66].copy_from_slice(&2u16.to_le_bytes());
    payload[offset + 66..offset + 68].copy_from_slice(&99u16.to_le_bytes());
    payload[offset + 68..offset + 70].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 70..offset + 72].fill(0);
    payload[offset + 72..offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(
        lane.sketch_entities
            .iter()
            .map(|entity| entity.local_id)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    assert_eq!(lane.sketch_entities[0].link_selector, Some(1));
    assert_eq!(
        lane.sketch_entities[0]
            .links
            .iter()
            .map(|link| (link.local_id, link.entity_ref.as_str()))
            .collect::<Vec<_>>(),
        [(2, lane.sketch_entities[1].id.as_str())]
    );
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn semantic_writer_rejects_edited_sketch_marker_local_id() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities[0].local_id = Some(7);
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("local object id does not match")));

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("inconsistent marker order"));
}

#[test]
fn semantic_writer_rejects_edited_sketch_marker_object_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities[0].object_index = Some(77);
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("object index does not match")));

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("inconsistent marker order"));
}

#[test]
fn semantic_writer_rejects_incomplete_sketch_marker_lanes() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1, 2],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities.remove(1);
    });
    decoded.source_fidelity_mut().annotations = cadmpeg_ir::Annotations::default();

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("has 3 markers but 2 native records"),
        "{error}"
    );
}

#[test]
fn native_validation_rejects_duplicate_sketch_marker_offsets() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let offset = native.feature_input_lanes[0].sketch_entities[0].offset;
        native.feature_input_lanes[0].sketch_entities[1].offset = offset;
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("repeats entity offset")));
}

#[test]
fn native_validation_requires_complete_ordered_sketch_markers() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1, 2],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities.remove(1);
        native.feature_input_lanes[0].sketch_entities[1].ordinal = 4;
    });
    let messages = crate::validate_native(decoded.ir())
        .into_iter()
        .map(|finding| finding.message)
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("expects entity ordinal")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits marker at offset")));
}
