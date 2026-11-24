// SPDX-License-Identifier: Apache-2.0

use crate::topology::{BodyKind, FaceLoops, RegionUnstated};

use super::without_partial_topology;

fn cube_with_partial_face() -> (crate::CadIr, String) {
    let mut ir = crate::examples::unit_cube().unwrap();
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
