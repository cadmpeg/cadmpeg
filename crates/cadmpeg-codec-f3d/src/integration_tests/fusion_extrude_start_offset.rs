// SPDX-License-Identifier: Apache-2.0
//! Signed start offsets of Fusion Extrude features.
//!
//! Each save extrudes a 2 mm New Body along +Z from a profile on the root XY
//! plane beside a plate that spans z 0..3.

use std::io::Cursor;

use cadmpeg_ir::features::extents::{ExtrudeDirection, ExtrudeStart};
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::loss::F3dLossCode;
use crate::F3dCodec;

/// The direction and start of the second Extrude. The decode charges no
/// assumed start-offset direction.
fn tool_extrude(bytes: &[u8]) -> (ExtrudeDirection, ExtrudeStart) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    assert!(
        !result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::FeatureExtrudeStartOffsetDirectionAssumed.kind()),
        "{:#?}",
        result.report().losses
    );
    let mut extrudes = result.ir().model.features.iter().filter_map(|feature| {
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction, start, ..
            }) => Some((direction.clone(), start.clone())),
            _ => None,
        }
    });
    extrudes.next().expect("the plate Extrude");
    let tool = extrudes.next().expect("the tool Extrude");
    assert!(extrudes.next().is_none());
    tool
}

fn assert_from_face(bytes: &[u8], expected: f64) {
    let (direction, start) = tool_extrude(bytes);
    assert_eq!(direction, ExtrudeDirection::ProfileNormal {});
    let ExtrudeStart::FromFace {
        face: FaceSelection::Historical { faces, .. },
        offset: Some(offset),
    } = start
    else {
        panic!("a selected-face start with one historical face: {start:?}");
    };
    assert_eq!(faces.as_slice().len(), 1);
    assert_eq!(offset.get(), expected);
}

fn assert_offset_profile_plane(bytes: &[u8], direction: &ExtrudeDirection, expected: f64) {
    let (decoded, start) = tool_extrude(bytes);
    assert_eq!(&decoded, direction);
    let ExtrudeStart::OffsetProfilePlane { offset } = start else {
        panic!("an offset profile-plane start: {start:?}");
    };
    assert_eq!(offset.get(), expected);
}

#[test]
fn a_start_offset_from_a_face_whose_outward_normal_opposes_the_travel_changes_sign() {
    // Start face: the plate's bottom face, outward normal -Z. Stored offset
    // -5 mm along that normal: the tool starts at z 5.
    assert_from_face(
        include_bytes!("../../../../corpus/f3d/extrude_from_face_bottom_minus5.f3d"),
        5.0,
    );
}

#[test]
fn a_start_offset_from_a_face_whose_outward_normal_follows_the_travel_keeps_its_sign() {
    // Start face: the plate's top face, outward normal +Z. Stored offset
    // -5 mm along that normal: the tool starts at z -2.
    assert_from_face(
        include_bytes!("../../../../corpus/f3d/extrude_from_face_top_minus5.f3d"),
        -5.0,
    );
}

#[test]
fn a_profile_plane_start_offset_runs_along_the_profile_normal() {
    assert_offset_profile_plane(
        include_bytes!("../../../../corpus/f3d/extrude_offset_start_plus5.f3d"),
        &ExtrudeDirection::ProfileNormal {},
        5.0,
    );
    assert_offset_profile_plane(
        include_bytes!("../../../../corpus/f3d/extrude_offset_start_plus5_reversed.f3d"),
        &ExtrudeDirection::ReversedProfileNormal {},
        5.0,
    );
    assert_offset_profile_plane(
        include_bytes!("../../../../corpus/f3d/extrude_offset_start_minus5.f3d"),
        &ExtrudeDirection::ProfileNormal {},
        -5.0,
    );
}
