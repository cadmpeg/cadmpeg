// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{LoopId, ShellId};

use super::super::BrepFaceReferences;

fn references_result() -> Result<BrepFaceReferences, CodecError> {
    BrepFaceReferences::from_loops(
        5,
        &ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1),
        2,
    )
}

#[test]
fn brep_face_references_preserve_service_loop_order() {
    let references = references_result().expect("build the face references");
    assert_eq!(references.face.as_str(), "creo:visibgeom:face#5");
    assert_eq!(references.shell_id.as_str(), "creo:visibgeom:shell#1");
    assert_eq!(
        references
            .loop_ids
            .iter()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
    assert_eq!(
        references
            .face_loops
            .iter()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
}
