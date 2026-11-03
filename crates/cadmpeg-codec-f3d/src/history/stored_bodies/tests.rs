// SPDX-License-Identifier: Apache-2.0

use super::ActiveBodies;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};

fn body(id: &str) -> Body {
    Body {
        id: BodyId::mint(id).unwrap(),
        kind: BodyKind::Wire,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    }
}

const PRIMARY: Option<&str> = Some("FusionAssetName[Active]/Breps.BlobParts/BREP.selected.smbh");

#[test]
fn saved_edge_matches_its_stable_slot_in_either_namespace() {
    for id in [
        "f3d:brep:saved-edge-body#7",
        "f3d:brep/selected.smbh/brep:saved-edge-body#7",
    ] {
        let bodies = [body(id)];
        let active = ActiveBodies::new(&bodies, PRIMARY);
        assert!(active.is_saved_edge(7, Some("selected.smbh")));
        assert!(!active.is_saved_edge(8, Some("selected.smbh")));
    }
}

#[test]
fn saved_edge_keeps_its_qualified_source() {
    let bodies = [
        body("f3d:brep/other.smbh/brep:saved-edge-body#7"),
        body("f3d:brep/selected.smbh/brep:saved-edge-body#7"),
    ];
    for ordered in [bodies.clone(), [bodies[1].clone(), bodies[0].clone()]] {
        let active = ActiveBodies::new(&ordered, None);
        assert!(active.is_saved_edge(7, Some("selected.smbh")));
        assert!(!active.is_saved_edge(7, Some("missing.smbh")));
        assert!(!active.is_saved_edge(7, None));
    }
}

#[test]
fn saved_edge_refuses_a_second_identity_at_its_slot() {
    let bodies = [
        body("f3d:brep:saved-edge-body#7"),
        body("f3d:brep/selected.smbh/brep:saved-edge-body#7"),
    ];
    assert!(!ActiveBodies::new(&bodies, PRIMARY).is_saved_edge(7, Some("selected.smbh")));
}

#[test]
fn saved_edge_requires_an_admitted_origin() {
    for id in ["f3d:brep:other#7", "other:brep:saved-edge-body#7"] {
        assert!(!ActiveBodies::new(&[body(id)], PRIMARY).is_saved_edge(7, Some("selected.smbh")));
    }
}

#[test]
fn saved_edge_requires_a_history_source() {
    let bodies = [body("f3d:brep:saved-edge-body#7")];
    for primary in [None, PRIMARY] {
        assert!(!ActiveBodies::new(&bodies, primary).is_saved_edge(7, None));
    }
}
