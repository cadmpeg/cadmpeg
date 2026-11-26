// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{LoopId, ShellId};

use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::FaceLoops;

use super::super::{state_face_loops, BrepFaceReferences};

fn references_result() -> Result<BrepFaceReferences, CodecError> {
    BrepFaceReferences::from_loops(
        5.into(),
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
            .expect("two loops are classified")
            .stated()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
}

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ))
}

#[test]
fn a_loopless_face_has_no_classified_loops() {
    let references = BrepFaceReferences::from_loops(
        7.into(),
        &ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1),
        0,
    )
    .expect("build the face references");
    assert!(references.loop_ids.is_empty());
    assert!(references.face_loops.is_none());
}

#[test]
fn a_loopless_face_on_a_plane_is_stated_partial_with_a_loss() {
    let references = BrepFaceReferences::from_loops(
        7.into(),
        &ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1),
        0,
    )
    .expect("build the face references");
    let mut losses = Vec::new();
    let loops = state_face_loops(
        references.face_loops,
        &references.face,
        ("VisibGeom", 7),
        Some(&plane()),
        &mut losses,
    );
    assert_eq!(loops, FaceLoops::PartialUnspecified { loops: Vec::new() });
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::TopologyFaceBoundaryUnstated
            .note("")
            .code
    );
    assert_eq!(
        losses[0].code.to_string(),
        "creo/topology.face-boundary-unstated"
    );
    assert!(
        losses[0].message.contains("creo:visibgeom:face#7"),
        "{}",
        losses[0].message
    );
}

#[test]
fn a_loopless_face_on_an_opaque_surface_keeps_its_form_without_a_loss() {
    let opaque = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let mut losses = Vec::new();
    let loops = state_face_loops(
        None,
        &crate::identity::compose_checked(&crate::identity::VISIBGEOM_FACE, 7)
            .expect("face identity"),
        ("VisibGeom", 7),
        Some(&opaque),
        &mut losses,
    );
    assert_eq!(loops, FaceLoops::unspecified(Vec::new()));
    assert!(losses.is_empty());
}
