// SPDX-License-Identifier: Apache-2.0

use super::shared_recovery::{assert_shared_sheet, transfer_with_notes};
use super::*;

fn offset_occurrence(accuracy: Option<f64>) -> Vec<Patch> {
    let mut patches = two_face_sheet();
    let patch = &mut patches[1];
    patch.surface = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.003),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("offset plane"),
    );
    for side in &mut patch.sides {
        side.curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            side.vertices
                .iter()
                .map(|(_, p)| Point3::new(p.x, p.y, 0.003))
                .collect(),
            None,
            false,
        )
        .expect("offset boundary");
    }
    patch.sides[3].inferred_accuracy =
        accuracy.map(|bound_mm| super::super::super::InferredEdgeAccuracy {
            source: 17,
            bound_mm,
        });
    patches
}

#[test]
fn planar_shared_edge_uses_native_accuracy_and_reports_the_lift() {
    let (ir, notes) = transfer_with_notes(offset_occurrence(Some(0.004)));
    assert_shared_sheet(&ir);
    let note = notes
        .iter()
        .find(|note| note.code.local_code() == "geometry.parameter-map-accuracy-inferred")
        .expect("native accuracy report");
    assert!(note.message.contains("1 emitted sheet faces"));
    assert!(note.message.contains("0.003000000 mm"));
    assert!(ir.model.edges.iter().any(|edge| edge
        .tolerance
        .is_some_and(|t| (t.get() - 0.005).abs() < 1.0e-12)));
}

#[test]
fn planar_shared_edge_rejects_missing_or_insufficient_native_accuracy() {
    for accuracy in [None, Some(0.0005)] {
        let (ir, notes) = transfer_with_notes(offset_occurrence(accuracy));
        assert_eq!(ir.model.faces.len(), 1);
        assert!(notes
            .iter()
            .all(|note| note.code.local_code() != "geometry.parameter-map-accuracy-inferred"));
    }
}

#[test]
fn ordinary_sheets_do_not_report_optional_recovery() {
    let (ir, notes) = transfer_with_notes(two_face_sheet());
    assert_shared_sheet(&ir);
    for code in [
        "geometry.rolling-sheet-extended",
        "topology.affine-vertex-reconciled",
        "geometry.parameter-map-accuracy-inferred",
    ] {
        assert!(notes.iter().all(|note| note.code.local_code() != code));
    }
}

#[test]
fn affine_reconciliation_metadata_reaches_the_emitted_loss() {
    let mut patches = two_face_sheet();
    patches[1].recovery.affine_vertex_shift_mm = 0.0098;
    let (ir, notes) = transfer_with_notes(patches);
    assert_shared_sheet(&ir);
    let note = notes
        .iter()
        .find(|note| note.code.local_code() == "topology.affine-vertex-reconciled")
        .expect("affine reconciliation loss");
    assert!(note.message.contains("1 emitted sheet faces"));
    assert!(note.message.contains("0.009800000 mm"));
}

#[test]
fn map_accuracy_and_affine_position_are_labelled_inferred() {
    let mut patches = offset_occurrence(Some(0.004));
    patches[0].recovery.affine_vertex_shift_mm = 0.0098;
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut annotations,
            patches,
            &mut LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("transfer inferred accuracy");
    let annotations = annotations.build();
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.tolerance.is_some_and(|t| t.get() > GAP_MM))
        .expect("widened edge");
    assert_eq!(
        annotations.exactness()[edge.id.as_str()].fields()["tolerance"],
        Exactness::Inferred
    );
    assert!(ir.model.points.iter().any(|point| annotations
        .exactness()
        .get(point.id.as_str())
        .is_some_and(|note| note.fields().get("position") == Some(&Exactness::Inferred))));
}

#[test]
fn rejected_components_emit_no_optional_recovery_notes() {
    let mut patches = two_face_sheet();
    patches.truncate(1);
    let patch = &mut patches[0];
    patch.sides[1].edge = patch.sides[0].edge;
    patch.recovery.rolling_extension_mm = 0.004;
    patch.recovery.affine_vertex_shift_mm = 0.0098;
    patch.sides[0].lift_error_mm = 0.003;
    patch.sides[0].inferred_accuracy = Some(super::super::super::InferredEdgeAccuracy {
        source: 17,
        bound_mm: 0.004,
    });
    let (ir, notes) = transfer_with_notes(patches);
    assert!(ir.model.faces.is_empty());
    for code in [
        "geometry.rolling-sheet-extended",
        "topology.affine-vertex-reconciled",
        "geometry.parameter-map-accuracy-inferred",
    ] {
        assert!(notes.iter().all(|note| note.code.local_code() != code));
    }
}
