// SPDX-License-Identifier: Apache-2.0
//! Compact edge and surface selection write-back rejection tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_rejects_compact_edge_selection_edits() {
    use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition};

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    {
        let mut ir_edit = decoded.ir_mut();
        update_sldprt_native(&mut ir_edit, |native| {
            let feature_ref = native.feature_histories[0].features[0].id.clone();
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
                &lane.native_payload,
                marker,
            )
            .unwrap();
            lane.edge_selections
                .push(crate::records::FeatureInputEdgeSelection {
                    id: "sldprt:test:edge-selection#0".into(),
                    parent: lane.id.clone(),
                    ordinal: 0,
                    offset: marker as u64,
                    object_name_ref: lane.names[0].id.clone(),
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
        let FeatureDefinition::Fillet { groups } = &mut feature.definition else {
            panic!("typed fillet");
        };
        groups[0].edges = EdgeSelection::Native("changed".into());
    }

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
            .contains("changes a compact edge selection"),
        "{error}"
    );
}

#[test]
fn semantic_writer_rejects_compact_surface_selection_edits() {
    use cadmpeg_ir::features::{ExtrudeExtent, FaceSelection, FeatureDefinition, Termination};

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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
            lane.native_payload.extend(6u32.to_le_bytes());
            lane.native_payload.extend([0x04, 0x02, 0, 0]);
            lane.native_payload.extend(0x1234u32.to_le_bytes());
            lane.native_payload.extend([
                0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49,
                0xb2, 0x54,
            ]);
            lane.native_payload.extend([0, 0]);
            lane.native_payload.extend(0x8c20u32.to_le_bytes());
            let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
            lane.native_payload.extend(signature);
            lane.native_payload.extend(12u32.to_le_bytes());
            lane.native_payload.extend([0; 24]);
            lane.surface_selections
                .push(crate::records::FeatureInputSurfaceSelection {
                    id: "sldprt:test:surface-selection#0".into(),
                    parent: lane.id.clone(),
                    ordinal: 0,
                    offset: marker as u64,
                    selector: 0,
                    endpoint_selector: None,
                    object_name_ref: lane
                        .names
                        .iter()
                        .find(|name| name.value == "UpTo")
                        .unwrap()
                        .id
                        .clone(),
                    feature_ref,
                    producer_feature_refs: Vec::new(),
                    terminal_feature_ref: None,
                    components: vec![crate::records::FeatureInputComponentPathEntry {
                        instance: Some(0x8c20),
                        type_signature: signature,
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
        let FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        } = &mut feature.definition
        else {
            panic!("typed extrusion");
        };
        let Termination::ToFace { face, .. } = &mut side.termination else {
            panic!("to-face termination");
        };
        *face = FaceSelection::Native("changed".into());
    }

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
            .contains("changes a compact surface selection"),
        "{error}"
    );
}
