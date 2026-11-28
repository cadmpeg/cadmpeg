// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of a Join whose participant bodies include a separate
//! New Body block.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The final body count and the history-only flag of each body-map pair.
fn bodies(bytes: &[u8]) -> (usize, Vec<bool>) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let native = crate::test_support::native_test::f3d_native(result.ir());
    let mut history_only = native
        .design_body_bindings
        .iter()
        .map(|binding| binding.history_only)
        .collect::<Vec<_>>();
    history_only.sort_unstable();
    (result.ir().geometry.bodies.len(), history_only)
}

#[test]
fn a_plate_and_a_touching_block_are_two_bodies() {
    // Fusion: Plate and Block, two New Body extrudes; 2 bodies.
    let decoded = bodies(include_bytes!(
        "../../../../corpus/f3d/join_absorbs_new_body_base.f3d"
    ));
    assert_eq!(decoded, (2, vec![false, false]));
}

#[test]
fn a_join_with_default_participants_merges_the_block_into_the_plate() {
    // Fusion: Extrude3 joins a boss that overlaps the Block and sits on the
    // Plate; its participants are all bodies. 1 body; Extrude2.bodies is empty.
    let decoded = bodies(include_bytes!(
        "../../../../corpus/f3d/join_absorbs_new_body.f3d"
    ));
    assert_eq!(decoded, (1, vec![false, true]));
}

#[test]
fn a_join_whose_participant_is_the_plate_keeps_the_block() {
    // Fusion: the same Join with participants [Plate]; 2 bodies.
    let decoded = bodies(include_bytes!(
        "../../../../corpus/f3d/join_absorbs_new_body_control.f3d"
    ));
    assert_eq!(decoded, (2, vec![false, false]));
}

#[test]
fn a_feature_mirror_of_the_block_and_the_join_adds_no_body() {
    // Fusion: the default-participant Join, then a feature Mirror of Extrude2
    // and Extrude3; 1 body, and Mirror1.bodies is [Plate].
    let decoded = bodies(include_bytes!(
        "../../../../corpus/f3d/join_absorbs_new_body_mirror.f3d"
    ));
    assert_eq!(decoded, (1, vec![false, true]));
}
