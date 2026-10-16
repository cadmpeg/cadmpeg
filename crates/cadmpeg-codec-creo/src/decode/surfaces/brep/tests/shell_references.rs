// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{FaceId, ShellId};

use super::super::{BrepShellReferences, NeutralShellSpec};

fn references_result() -> Result<BrepShellReferences, CodecError> {
    let shell = NeutralShellSpec {
        faces: vec![5],
        wire_curves: BTreeSet::from([10]),
    };
    let shell_id = ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1);
    BrepShellReferences::from_shell(&shell, &shell_id, &mut BTreeMap::new())
}

#[test]
fn brep_shell_references_preserve_service_order() {
    let references = references_result().expect("build the shell references");
    assert_eq!(
        references.face_ids,
        vec![FaceId::compose(&crate::identity::VISIBGEOM_FACE, 5)]
    );
    assert_eq!(references.edge_ids.len(), 1);
    assert_eq!(references.edge_ids[0].as_str(), "creo:visibgeom:edge#10");
}
