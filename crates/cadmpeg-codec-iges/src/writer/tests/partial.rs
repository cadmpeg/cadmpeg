// SPDX-License-Identifier: Apache-2.0
//! A partial face is left out, and its solid is written as a shell in no
//! manifold solid (§3.4, Content the target cannot hold).

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::{FaceLoops, RegionUnstated};

use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use crate::{IgesCodec, IgesVersion};

/// A tetrahedron solid as a document of another codec states it: with no
/// IGES native records.
pub(super) fn tetrahedron() -> cadmpeg_ir::CadIr {
    let mut ir = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("decode")
        .ir()
        .clone();
    ir.native = cadmpeg_ir::native::Native::default();
    ir
}

fn messages(synthesis: &crate::writer::Synthesis) -> Vec<&str> {
    synthesis
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn a_solid_with_a_partial_face_writes_its_complete_faces_as_a_shell() {
    let mut ir = tetrahedron();
    let face = &mut ir.geometry.faces[0];
    let loops = face.loops.stated().cloned().collect();
    face.loops = FaceLoops::PartialUnspecified { loops };
    let partial = face.id.as_str().to_owned();

    let written = crate::writer::synthesize(
        &ir,
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
    .expect("synthesis");
    assert_eq!(written.counts.get("186_manifold_solid_brep"), None);
    assert_eq!(written.counts.get("514_shell"), Some(&1));
    assert_eq!(written.counts.get("510_face"), Some(&3));
    let messages = messages(&written);
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("IGES writes no partial face")
                && message.contains(&partial)),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("IGES writes no solid for")),
        "{messages:?}"
    );
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written.bytes), &DecodeOptions::default())
        .expect("written IGES decodes");
    assert_eq!(round_trip.ir().geometry.faces.len(), 3);
}

#[test]
fn a_region_whose_voids_are_unstated_keeps_every_face() {
    let mut ir = tetrahedron();
    ir.geometry.regions[0].unstated = Some(RegionUnstated::Voids);
    let written = crate::writer::synthesize(
        &ir,
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
    .expect("synthesis");
    assert_eq!(written.counts.get("186_manifold_solid_brep"), None);
    assert_eq!(written.counts.get("510_face"), Some(&4));
}

#[test]
fn a_complete_solid_is_still_a_manifold_solid() {
    let written = crate::writer::synthesize(
        &tetrahedron(),
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
    .expect("synthesis");
    assert_eq!(written.counts.get("186_manifold_solid_brep"), Some(&1));
    assert!(!messages(&written)
        .iter()
        .any(|message| message.contains("partial") || message.contains("writes no solid")));
}
