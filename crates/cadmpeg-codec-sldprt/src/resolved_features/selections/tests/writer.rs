// SPDX-License-Identifier: Apache-2.0
//! Compact edge and surface selection write-back rejection tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::native::update_sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn semantic_writer_synthesizes_compact_edge_selection_edits() {
    use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};

    let reference_arena = cadmpeg_core::decode::DecodeArena::new();
    let (reference_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &reference_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Round" Type="Fillet" id="41" Edges="edge:1"><Dimension Name="Radius">2mm</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        update_sldprt_native(&mut ir_edit, |native| {
            let feature_ref = native.feature_histories[0].features[0].id.clone();
            let histories = native.feature_histories.clone();
            let lane = &mut native.feature_input_lanes[0];
            let marker = lane.native_payload.len() + 12;
            lane.native_payload.extend(1u32.to_le_bytes());
            lane.native_payload
                .extend([0x00, 0x02, 0x00, 0x00, 0, 0, 0, 0]);
            lane.native_payload.extend([
                0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49,
                0xb2, 0x54,
            ]);
            lane.native_payload.extend([0, 0]);
            lane.native_payload.extend(0x818bu32.to_le_bytes());
            lane.native_payload.extend([
                0x00, 0x81, 0x03, 0x01, 0x2c, 0, 0, 0, 0x63, 0x18, 0x58, 0x69,
            ]);
            lane.native_payload.extend(7u32.to_le_bytes());
            let components = crate::resolved_features::selections::compact_edge_component_path_at(
                &reference_ctx,
                &lane.native_payload,
                marker,
            )
            .unwrap()
            .unwrap();
            // The pushed selection states the bytes extended above, so it
            // carries the deterministic identity the payload parses to (#226
            // lane agreement). Only the edited edge set is test input.
            let parsed = crate::resolved_features::write_prepare::generated_native_lane(
                &histories,
                &lane.id.clone(),
                lane.configuration.clone(),
                lane.native_payload.to_vec(),
            )
            .expect("extended lane payload parses");
            let parsed_selection = parsed
                .edge_selections
                .first()
                .expect("extended payload holds one edge selection");
            let (selection_id, selection_sequence) =
                (parsed_selection.id.clone(), parsed_selection.sequence);
            let (lane_id, name_ref) = (lane.id.clone(), lane.names[0].id.clone());
            lane.edge_selections
                .push(crate::records::FeatureInputEdgeSelection {
                    id: selection_id,
                    parent: lane_id,
                    ordinal: 0,
                    sequence: selection_sequence,
                    object_name_ref: name_ref,
                    feature_ref,
                    local_edge_ids: vec![7],
                    components,
                    references: Vec::new(),
                    producer_feature_refs: Vec::new(),
                    terminal_feature_ref: None,
                });
        });
        let feature = ir_edit
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Round"))
            .unwrap();
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition
            else {
                panic!("typed fillet");
            };
            groups[0].edges = EdgeSelection::Native("changed".into());
        });
    }

    let reason =
        crate::test_support::assert_synthesis_fallback(decoded.ir(), decoded.source_fidelity());
    assert!(
        reason.contains("changes a compact edge selection"),
        "{reason}"
    );
}

#[test]
fn semantic_writer_synthesizes_compact_surface_selection_edits() {
    use cadmpeg_ir::features::{
        ExtrudeExtent, FaceSelection, FeatureDefinition, FeatureOperation, LinearTermination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="30"/><Extrusion Name="UpTo" Type="BossExtrude" id="31" Profile="30" EndCondition="ToFace" Face="face:12" Operation="Join"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moExtrusion_c", "UpTo", 31)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        update_sldprt_native(&mut ir_edit, |native| {
            let feature_ref = native.feature_histories[0]
                .features
                .iter()
                .find(|feature| feature.name == "UpTo")
                .unwrap()
                .id
                .clone();
            let lane = &mut native.feature_input_lanes[0];
            let marker = lane.native_payload.len() + 12;
            // The edited selection is carried without payload bytes: a lane
            // without payload bytes passes agreement, and the patch path
            // declines on the document/selection disagreement (#237 fallback).
            lane.native_payload.clear();
            let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
            // The pushed selection states the edited surface set, so it
            // carries placeholder identity (#226 lane agreement passes a lane
            // without payload bytes). Only the edited surface set is input.
            let (lane_id, name_ref) = (
                lane.id.clone(),
                lane.names
                    .iter()
                    .find(|name| name.value == "UpTo")
                    .unwrap()
                    .id
                    .clone(),
            );
            lane.surface_selections
                .push(crate::records::FeatureInputSurfaceSelection {
                    id: "sldprt:test:surface-selection#0".into(),
                    parent: lane_id,
                    ordinal: 0,
                    sequence: 0,
                    offset: cadmpeg_core::decode::u64_from_index(marker),
                    selector: 0,
                    kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
                    object_name_ref: name_ref,
                    feature_ref,
                    producer_feature_refs: Vec::new(),
                    terminal_feature_ref: None,
                    components: vec![crate::records::FeatureInputComponentPathEntry {
                        instance: Some(0x8c20),
                        type_signature: signature.into(),
                        local_id: Some(12),
                    }],
                });
        });
        let feature = ir_edit
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("UpTo"))
            .unwrap();
        let updated_feature_evaluation = &mut feature.evaluation;
        let mut updated_feature_definition = updated_feature_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        }) = &mut updated_feature_definition
        else {
            panic!("typed extrusion");
        };
        let LinearTermination::ToFace { face, .. } = &mut side.termination else {
            panic!("to-face termination");
        };
        *face = FaceSelection::Native("changed".into());
        updated_feature_evaluation.set_definition(updated_feature_definition);
    }

    let reason =
        crate::test_support::assert_synthesis_fallback(decoded.ir(), decoded.source_fidelity());
    assert!(
        reason.contains("changes a compact surface selection"),
        "{reason}"
    );
}
