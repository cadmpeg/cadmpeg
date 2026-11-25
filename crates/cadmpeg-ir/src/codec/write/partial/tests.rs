// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::topology::{BodyKind, FaceLoops, RegionUnstated};

use super::{without_faces_and_bodies, without_partial_topology};

fn cube_with_partial_face() -> (crate::CadIr, String) {
    let mut ir = crate::examples::unit_cube().unwrap();
    crate::examples::add_document_root(&mut ir);
    let face = &mut ir.geometry.faces[0];
    let loops = face.loops.stated().cloned().collect();
    face.loops = FaceLoops::PartialUnspecified { loops };
    let id = face.id.as_str().to_owned();
    (ir, id)
}

fn messages(losses: &[crate::report::loss::LossNote]) -> Vec<&str> {
    losses.iter().map(|loss| loss.message.as_str()).collect()
}

#[test]
fn complete_topology_needs_no_other_form() {
    let ir = crate::examples::unit_cube().unwrap();
    assert!(without_partial_topology(&ir, "TEST", "shells")
        .unwrap()
        .is_none());
}

#[test]
fn a_partial_face_is_left_out_and_its_solid_is_written_as_a_valid_sheet() {
    let (ir, partial) = cube_with_partial_face();
    let (written, losses) = without_partial_topology(&ir, "TEST", "shells")
        .unwrap()
        .unwrap();
    let geometry = &written.geometry;
    assert_eq!(geometry.faces.len(), ir.geometry.faces.len() - 1);
    assert!(geometry
        .faces
        .iter()
        .all(|face| face.id.as_str() != partial && !face.loops.is_partial()));
    assert_eq!(geometry.loops.len(), ir.geometry.loops.len() - 1);
    assert_eq!(geometry.coedges.len(), ir.geometry.coedges.len() - 4);
    assert_eq!(geometry.edges.len(), ir.geometry.edges.len());
    assert_eq!(geometry.surfaces.len(), ir.geometry.surfaces.len() - 1);
    assert_eq!(geometry.bodies[0].kind, BodyKind::Sheet);
    let report = crate::validate::validate_neutral(&written, Vec::new()).unwrap();
    assert!(report.is_ok(), "{:?}", report.findings);
    let messages = messages(&losses);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages[0].starts_with("TEST writes no partial face") && messages[0].contains(&partial)
    );
    assert!(messages[1].starts_with("TEST writes no solid for"));
}

#[test]
fn a_marked_region_lists_its_shells_and_keeps_every_face() {
    let mut ir = crate::examples::unit_cube().unwrap();
    crate::examples::add_document_root(&mut ir);
    ir.geometry.regions[0].unstated = Some(RegionUnstated::Voids);
    let (written, losses) = without_partial_topology(&ir, "TEST", "shells")
        .unwrap()
        .unwrap();
    let geometry = &written.geometry;
    assert!(geometry.regions[0].unstated.is_none());
    assert_eq!(
        geometry.regions[0].stated_shells().collect::<Vec<_>>(),
        ir.geometry.regions[0].stated_shells().collect::<Vec<_>>()
    );
    assert_eq!(geometry.faces.len(), ir.geometry.faces.len());
    assert_eq!(geometry.bodies[0].kind, BodyKind::Sheet);
    let report = crate::validate::validate_neutral(&written, Vec::new()).unwrap();
    assert!(report.is_ok(), "{:?}", report.findings);
    assert_eq!(messages(&losses).len(), 1);
}

#[test]
fn a_dropped_face_opens_its_solid_into_a_valid_sheet() {
    let mut ir = crate::examples::unit_cube().unwrap();
    crate::examples::add_document_root(&mut ir);
    let dropped = ir.geometry.faces[0].id.as_str().to_owned();
    let written =
        without_faces_and_bodies(&ir, &BTreeSet::from([dropped.as_str()]), &BTreeSet::new())
            .unwrap();
    let geometry = &written.geometry;
    assert_eq!(geometry.faces.len(), ir.geometry.faces.len() - 1);
    assert!(geometry
        .faces
        .iter()
        .all(|face| face.id.as_str() != dropped));
    assert_eq!(geometry.loops.len(), ir.geometry.loops.len() - 1);
    assert_eq!(geometry.coedges.len(), ir.geometry.coedges.len() - 4);
    assert_eq!(geometry.edges.len(), ir.geometry.edges.len());
    assert_eq!(geometry.surfaces.len(), ir.geometry.surfaces.len() - 1);
    assert_eq!(geometry.shells.len(), ir.geometry.shells.len());
    assert_eq!(geometry.bodies[0].kind, BodyKind::Sheet);
    let report = crate::validate::validate_neutral(&written, Vec::new()).unwrap();
    assert!(report.is_ok(), "{:?}", report.findings);
}

#[test]
fn dropping_every_face_leaves_out_the_shell_region_body_and_carriers() {
    let mut ir = crate::examples::unit_cube().unwrap();
    crate::examples::add_document_root(&mut ir);
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .collect::<BTreeSet<_>>();
    let written = without_faces_and_bodies(&ir, &faces, &BTreeSet::new()).unwrap();
    let geometry = &written.geometry;
    assert!(geometry.bodies.is_empty());
    assert!(geometry.regions.is_empty());
    assert!(geometry.shells.is_empty());
    assert!(geometry.faces.is_empty());
    assert!(geometry.edges.is_empty());
    assert!(geometry.vertices.is_empty());
    assert!(geometry.surfaces.is_empty());
    assert!(geometry.curves.is_empty());
}

#[test]
fn a_dropped_body_takes_its_topology_and_carriers() {
    let mut ir = crate::examples::unit_cube().unwrap();
    crate::examples::add_document_root(&mut ir);
    let body = ir.geometry.bodies[0].id.as_str().to_owned();
    let written =
        without_faces_and_bodies(&ir, &BTreeSet::new(), &BTreeSet::from([body.as_str()])).unwrap();
    let geometry = &written.geometry;
    assert!(geometry.bodies.is_empty());
    assert!(geometry.regions.is_empty());
    assert!(geometry.shells.is_empty());
    assert!(geometry.faces.is_empty());
    assert!(geometry.loops.is_empty());
    assert!(geometry.coedges.is_empty());
    assert!(geometry.edges.is_empty());
    assert!(geometry.points.is_empty());
}
