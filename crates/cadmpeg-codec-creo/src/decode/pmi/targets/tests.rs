// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::pmi::PmiTarget;

use super::{dimension_targets_by_slot, DimensionTarget};

fn face(id: &str) -> PmiTarget {
    PmiTarget::Face {
        face: FaceId::try_from(format!("creo:visibgeom:face#{id}")).expect("face id"),
    }
}

fn target(faces: &[&str]) -> DimensionTarget {
    DimensionTarget {
        faces: faces.iter().map(|id| face(id)).collect(),
        from_edges: false,
    }
}

#[test]
fn slot_targets_need_agreement_across_sections() {
    let targets = BTreeMap::from([
        (("BasFullData", 417), target(&["506"])),
        (("FullMData", 417), target(&["506"])),
        (("BasFullData", 418), target(&["1"])),
        (("FullMData", 418), target(&["2"])),
    ]);
    let by_slot = dimension_targets_by_slot(&targets);
    assert_eq!(by_slot.get(&417), Some(&vec![face("506")]));
    assert_eq!(by_slot.get(&418), None);
}

/// An IR whose edge `id` bounds each listed face through one coedge.
fn edge_ir(edges: &[(u32, &[u32])]) -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId};
    use cadmpeg_ir::topology::{Coedge, Loop, LoopBoundary, LoopRing, Sense};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut rings = BTreeMap::<u32, Vec<CoedgeId>>::new();
    for (edge, faces) in edges {
        for face in *faces {
            let coedge = CoedgeId::try_from(format!("creo:visibgeom:coedge#{edge}:{face}"))
                .expect("coedge id");
            let owner_loop =
                LoopId::try_from(format!("creo:visibgeom:loop#{face}")).expect("loop id");
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop,
                edge: EdgeId::try_from(format!("creo:visibgeom:edge#{edge}")).expect("edge id"),
                radial_next: coedge.clone(),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            });
            rings.entry(*face).or_default().push(coedge);
        }
    }
    for (face, coedges) in rings {
        ir.model.loops.push(Loop {
            id: LoopId::try_from(format!("creo:visibgeom:loop#{face}")).expect("loop id"),
            face: FaceId::try_from(format!("creo:visibgeom:face#{face}")).expect("face id"),
            boundary: LoopBoundary::Ring(LoopRing::new(coedges, Vec::new()).expect("loop ring")),
        });
    }
    ir
}

#[test]
fn two_edge_links_select_each_edge_face_the_other_edge_does_not_bound() {
    let ir = edge_ir(&[(1, &[10, 11]), (2, &[10, 12])]);
    let target = super::resolve_dimension(&[(1, 1), (1, 2)], &ir).expect("edge target");
    assert!(target.from_edges);
    assert_eq!(target.faces, vec![face("11"), face("12")]);
}

#[test]
fn edge_links_without_a_shared_face_select_no_target() {
    let ir = edge_ir(&[(1, &[10]), (2, &[11])]);
    assert!(super::resolve_dimension(&[(1, 1), (1, 2)], &ir).is_none());
}

#[test]
fn an_edge_with_two_unshared_faces_selects_no_target() {
    let ir = edge_ir(&[(1, &[10, 11, 14]), (2, &[10, 12])]);
    assert!(super::resolve_dimension(&[(1, 1), (1, 2)], &ir).is_none());
}

#[test]
fn one_edge_linked_twice_selects_no_target() {
    let ir = edge_ir(&[(1, &[10, 11])]);
    assert!(super::resolve_dimension(&[(1, 1), (1, 1)], &ir).is_none());
}
