// SPDX-License-Identifier: Apache-2.0
//! A partial face is left out, and the complete faces of its body are written
//! as an open Brep (§3.4, Content the target cannot hold).

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::{BodyKind, FaceLoops, RegionUnstated};

use super::planar_tetrahedron;
use crate::{RhinoArchiveVersion, RhinoCodec};

fn write(ir: &cadmpeg_ir::CadIr) -> Result<(Vec<u8>, Vec<String>), cadmpeg_core::CodecError> {
    let mut bytes = Vec::new();
    let plan = RhinoCodec.plan(
        EncodeInput::new(ir, None),
        TargetRequest::Explicit(RhinoArchiveVersion::V7.descriptor().id.as_str()),
    )?;
    let report = plan.write_to(&mut bytes)?;
    let messages = report
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect();
    Ok((bytes, messages))
}

#[test]
fn a_partial_face_is_left_out_of_an_open_brep() {
    let mut ir = planar_tetrahedron();
    let face = &mut ir.geometry.faces[0];
    let loops = face.loops.stated().cloned().collect();
    face.loops = FaceLoops::PartialUnspecified { loops };
    let partial = face.id.as_str().to_owned();

    let (bytes, losses) = write(&ir).expect("a partial face no longer refuses the write");
    assert!(
        losses
            .iter()
            .any(|loss| loss.starts_with("Rhino writes no partial face") && loss.contains(&partial)),
        "{losses:?}"
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss.starts_with("Rhino writes no solid for")),
        "{losses:?}"
    );
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written archive decodes");
    let geometry = &decoded.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1);
    assert_eq!(geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(geometry.faces.len(), 3);
}

#[test]
fn a_marked_region_with_no_partial_face_is_written_as_one_sheet_per_face() {
    let mut ir = planar_tetrahedron();
    ir.geometry.regions[0].unstated = Some(RegionUnstated::Voids);
    let body = ir.geometry.bodies[0].id.as_str().to_owned();

    let (decoded, losses) = super::written(&ir);
    let geometry = &decoded.geometry;
    assert_eq!(geometry.bodies.len(), 4);
    assert!(geometry
        .bodies
        .iter()
        .all(|body| body.kind == BodyKind::Sheet));
    assert_eq!(geometry.faces.len(), 4);
    assert!(
        losses
            .iter()
            .any(|loss| loss.starts_with("rhino/writer.body-split") && loss.contains(&body)),
        "{losses:?}"
    );
}
