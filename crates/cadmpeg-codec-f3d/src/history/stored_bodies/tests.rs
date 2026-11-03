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

fn solid(id: &str) -> Body {
    Body {
        kind: BodyKind::Solid,
        ..body(id)
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

#[test]
fn a_slot_is_a_saved_edge_or_a_result_body_and_never_both() {
    let bodies = [
        body("f3d:brep:saved-edge-body#7"),
        body("f3d:brep/selected.smbh/brep:saved-edge-body#8"),
        solid("f3d:brep:entity#9"),
        solid("f3d:brep/selected.smbh/brep:entity#10"),
    ];
    let active = ActiveBodies::new(&bodies, PRIMARY);
    let source = Some("selected.smbh");
    for slot in [7, 8] {
        assert!(active.is_saved_edge(slot, source));
        assert!(active.resolve(&[slot], source).is_none());
    }
    for slot in [9, 10] {
        assert!(!active.is_saved_edge(slot, source));
        assert!(active.resolve(&[slot], source).is_some());
    }
}

#[test]
fn a_solid_and_a_saved_edge_at_one_slot_of_the_primary_source_stay_unresolved() {
    let bodies = [
        solid("f3d:brep:entity#5"),
        body("f3d:brep:saved-edge-body#5"),
    ];
    for ordered in [bodies.clone(), [bodies[1].clone(), bodies[0].clone()]] {
        let active = ActiveBodies::new(&ordered, PRIMARY);
        assert!(!active.is_saved_edge(5, Some("selected.smbh")));
        assert!(active.resolve(&[5], Some("selected.smbh")).is_none());
    }
}

#[test]
fn a_saved_edge_of_the_primary_source_does_not_join_another_source() {
    let bodies = [
        solid("f3d:brep:entity#5"),
        body("f3d:brep:saved-edge-body#5"),
    ];
    let active = ActiveBodies::new(&bodies, PRIMARY);
    assert!(!active.is_saved_edge(5, Some("other.smbh")));
    let resolved = active.resolve(&[5], Some("other.smbh")).unwrap();
    assert_eq!(resolved.as_slice(), std::slice::from_ref(&bodies[0].id));
}

#[test]
fn a_saved_edge_is_never_a_result_body_without_a_source() {
    let bodies = [body("f3d:brep:saved-edge-body#7")];
    assert!(ActiveBodies::new(&bodies, None)
        .resolve(&[7], None)
        .is_none());
}
