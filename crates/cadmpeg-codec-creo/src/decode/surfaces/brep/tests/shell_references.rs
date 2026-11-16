// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::ids::ShellId;

use super::super::{BrepShellReferences, NeutralShellSpec};

#[test]
fn brep_shell_references_preserve_service_order() {
    let shell = NeutralShellSpec {
        faces: vec![5.into()],
        wire_curves: BTreeSet::from([10]),
    };
    let shell_id = ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1);
    let mut face_shell_ids = BTreeMap::new();
    let references = BrepShellReferences::from_shell(&shell, &shell_id, &mut face_shell_ids)
        .expect("build the shell references");
    assert_eq!(face_shell_ids.get(&5.into()), Some(&shell_id));
    assert_eq!(references.edge_ids.len(), 1);
    assert_eq!(references.edge_ids[0].as_str(), "creo:visibgeom:edge#10");
}
