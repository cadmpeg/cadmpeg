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
    let target = super::resolve_dimension(&[(1, 1), (1, 2)], &ir, "Dimension 219:1")
        .0
        .expect("edge target");
    assert!(target.from_edges);
    assert_eq!(target.faces, vec![face("11"), face("12")]);
}

#[test]
fn edge_links_without_a_shared_face_select_no_target() {
    let ir = edge_ir(&[(1, &[10]), (2, &[11])]);
    assert!(
        super::resolve_dimension(&[(1, 1), (1, 2)], &ir, "Dimension 219:1")
            .0
            .is_none()
    );
}

#[test]
fn an_edge_with_two_unshared_faces_selects_no_target() {
    let ir = edge_ir(&[(1, &[10, 11, 14]), (2, &[10, 12])]);
    assert!(
        super::resolve_dimension(&[(1, 1), (1, 2)], &ir, "Dimension 219:1")
            .0
            .is_none()
    );
}

#[test]
fn one_edge_linked_twice_selects_no_target() {
    let ir = edge_ir(&[(1, &[10, 11])]);
    assert!(
        super::resolve_dimension(&[(1, 1), (1, 1)], &ir, "Dimension 219:1")
            .0
            .is_none()
    );
}

fn cylinder_surface(id: &str, center_x: f64) -> cadmpeg_ir::geometry::Surface {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    Surface {
        id: cadmpeg_ir::ids::SurfaceId::try_from(id.to_owned()).expect("surface id"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(center_x, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                17.5,
            )
            .expect("cylinder"),
        )),
        source_object: None,
    }
}

fn visible_face(ir: &mut cadmpeg_ir::document::CadIr, face_id: &str, surface: &str) {
    use cadmpeg_ir::ids::{ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

    ir.model.faces.push(Face {
        id: FaceId::try_from(format!("creo:visibgeom:face#{face_id}")).expect("face id"),
        shell: ShellId::try_from("creo:visibgeom:shell#1".to_owned()).expect("shell id"),
        surface: SurfaceId::try_from(surface.to_owned()).expect("surface id"),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    });
}

#[test]
fn a_surface_link_selects_the_faces_of_its_visible_surface() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .push(cylinder_surface("creo:visibgeom:surface#7", 0.0));
    visible_face(&mut ir, "7", "creo:visibgeom:surface#7");
    let target = super::resolve_dimension(&[(0x25, 7), (0x25, 7)], &ir, "Dimension 219:1")
        .0
        .expect("target");
    assert_eq!(target.faces, vec![face("7")]);
}

#[test]
fn a_datum_cylinder_link_selects_the_visible_faces_on_its_carrier() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .push(cylinder_surface("creo:actdatums:surface#7", 0.0));
    ir.model
        .surfaces
        .push(cylinder_surface("creo:visibgeom:surface#8", 0.0));
    ir.model
        .surfaces
        .push(cylinder_surface("creo:visibgeom:surface#9", 50.0));
    visible_face(&mut ir, "8", "creo:visibgeom:surface#8");
    visible_face(&mut ir, "9", "creo:visibgeom:surface#9");
    let target = super::resolve_dimension(&[(0x25, 7), (0x25, 7)], &ir, "Dimension 219:1")
        .0
        .expect("target");
    assert_eq!(target.faces, vec![face("8")]);
}

#[test]
fn a_link_that_names_a_visible_and_a_datum_surface_selects_no_target() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .push(cylinder_surface("creo:actdatums:surface#7", 0.0));
    ir.model
        .surfaces
        .push(cylinder_surface("creo:visibgeom:surface#7", 50.0));
    visible_face(&mut ir, "7", "creo:visibgeom:surface#7");
    let (target, notes) = super::resolve_dimension(&[(0x25, 7), (0x25, 7)], &ir, "Dimension 219:1");
    assert!(target.is_none());
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].code,
        crate::loss::CreoLossCode::PmiTargetNamespaceAmbiguous.kind()
    );
}

/// An IR where surface number 7 names a visible surface with one face and,
/// when `datum` holds, also an `ActDatums` surface.
fn tolerance_surface_ir(datum: bool) -> cadmpeg_ir::document::CadIr {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    if datum {
        ir.model
            .surfaces
            .push(cylinder_surface("creo:actdatums:surface#7", 0.0));
    }
    ir.model
        .surfaces
        .push(cylinder_surface("creo:visibgeom:surface#7", 50.0));
    visible_face(&mut ir, "7", "creo:visibgeom:surface#7");
    ir
}

#[test]
fn a_tolerance_surface_link_selects_the_unique_visible_face() {
    let (target, note) =
        super::surface_tolerance_target(&tolerance_surface_ir(false), 7, (3900, 219));
    assert!(matches!(
        target,
        Some(super::ToleranceTarget::Surface(selected)) if selected == face("7")
    ));
    assert!(note.is_none());
}

#[test]
fn a_tolerance_link_that_names_both_sections_keeps_the_visible_face_and_charges_the_loss() {
    let (target, note) =
        super::surface_tolerance_target(&tolerance_surface_ir(true), 7, (3900, 219));
    assert!(matches!(
        target,
        Some(super::ToleranceTarget::Surface(selected)) if selected == face("7")
    ));
    assert_eq!(
        note.expect("ambiguity note").code,
        crate::loss::CreoLossCode::PmiTargetNamespaceAmbiguous.kind()
    );
}

#[test]
fn a_tolerance_link_that_names_both_sections_without_a_unique_face_is_withheld() {
    let mut ir = tolerance_surface_ir(true);
    visible_face(&mut ir, "8", "creo:visibgeom:surface#7");
    let (target, note) = super::surface_tolerance_target(&ir, 7, (3900, 219));
    assert!(target.is_none());
    assert_eq!(
        note.expect("ambiguity note").code,
        crate::loss::CreoLossCode::PmiTargetNamespaceAmbiguous.kind()
    );
}
