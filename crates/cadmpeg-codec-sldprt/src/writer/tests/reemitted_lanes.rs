// SPDX-License-Identifier: Apache-2.0
//! Feature-input lanes re-emitted by the writer keep the bytes it does not edit.
#![allow(clippy::unwrap_used)]

use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};

/// A lane holding one point marker at 150.
fn lane(key: u64) -> FeatureInputLane {
    let id = format!("sldprt:feature-input:config-objects#{key}");
    let mut marker = SketchInputEntity::new(
        format!("sldprt:feature-input:sketch-entity#{key}:150"),
        &id,
        0,
        150,
        SketchInputKind::Point,
    );
    marker.feature_ref = None;
    // The marker's bytes, as `SketchInputEntity::new` lays them out.
    let mut payload = vec![0; 512];
    payload[146..150].fill(0xff);
    payload[150..155].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    payload[155..163].fill(0xff);
    payload[163..167].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    FeatureInputLane {
        id,
        configuration: Some("0".into()),
        native_payload: payload.into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![marker],
    }
}

/// A re-emitted lane keeps a marker's own code where decode only reclassified
/// the marker: a code-1 record that decode reads as a `Point` stays 1, so an
/// edit elsewhere in the lane changes no other marker's bytes.
#[test]
fn a_reemitted_lane_keeps_a_reclassified_markers_code() {
    let mut lane = lane(4364);
    let mut payload = lane.native_payload.to_vec();
    payload[167..171].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload = payload.into();
    let written = crate::writer::resolved_feature_payload(
        &lane,
        &[],
        &[],
        crate::writer::ClassLayouts::Authored,
        &crate::resolved_features::class_sources::ClassSources::default(),
    )
    .map(|(payload, _)| payload)
    .unwrap();
    assert_eq!(&written[167..171], &1u32.to_le_bytes());
}

/// A re-emitted lane leaves a marker's unchanged coordinates where its layout
/// keeps them. This point record keeps its pair at +58
/// and +66; it is not rewritten at +66 and +74.
#[test]
fn a_reemitted_lane_keeps_unchanged_coordinates_in_their_layout() {
    // From four bytes before the marker to past the next marker.
    const RECORD: &str = "01000000ffff1f0003ffffffffffffffff000080bf0200000000000400020001000000000080bf00000400000000000000000000000000000000f03f1e001904560e2db29dbff97e6abc7493883f00000200ffff01000c0073674c696e6548616e646c650100ffffffff0000000019810200ffffffff000000000000feffffff00000000000000000000000000000000000000000000000000000000000000000000000000000000000002000000ffff1f0003ffffffffffffffff000080bf0000000000000400020001000000000080bf0000040000";
    let record = (0..RECORD.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&RECORD[at..at + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let mut lane = lane(4364);
    let mut payload = lane.native_payload.to_vec();
    payload[146..146 + record.len()].copy_from_slice(&record);
    let coordinates =
        crate::resolved_features::markers::marker_coordinates(&payload, 150).expect("coordinates");
    assert_eq!(&payload[150 + 58..150 + 66], &coordinates[0].to_le_bytes());
    let (staged, _) = crate::resolved_features::write_prepare::generated_native_lane(
        &[],
        &lane.id,
        lane.configuration.clone(),
        payload.clone(),
    )
    .unwrap();
    let mut markers = staged.sketch_entities;
    markers[0].feature_ref = None;
    markers[0].coordinates_m = Some(coordinates);
    lane.sketch_entities = markers;
    lane.classes = staged.classes;
    lane.native_payload = payload.clone().into();
    let written = crate::writer::resolved_feature_payload(
        &lane,
        &[],
        &[],
        crate::writer::ClassLayouts::Authored,
        &crate::resolved_features::class_sources::ClassSources::default(),
    )
    .map(|(payload, _)| payload)
    .unwrap();
    assert_eq!(&written[150 + 48..150 + 170], &payload[150 + 48..150 + 170]);
    // A change to coordinates this layout keeps elsewhere is refused, not
    // written at +66 and +74.
    let mut edited = lane.clone();
    edited.sketch_entities[0].coordinates_m =
        cadmpeg_ir::units::FiniteVector::new([coordinates[0] + 0.001, coordinates[1]]);
    let error = crate::writer::resolved_feature_payload(
        &edited,
        &[],
        &[],
        crate::writer::ClassLayouts::Authored,
        &crate::resolved_features::class_sources::ClassSources::default(),
    )
    .map(|(payload, _)| payload)
    .unwrap_err();
    assert!(
        error.to_string().contains("keeps them elsewhere"),
        "{error}"
    );
}
