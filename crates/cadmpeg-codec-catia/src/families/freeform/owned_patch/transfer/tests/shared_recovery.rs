// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::math::Point2;

fn transfer(patches: Vec<Patch>) -> CadIr {
    transfer_with_notes(patches).0
}

pub(super) fn transfer_with_notes(patches: Vec<Patch>) -> (CadIr, Vec<LossNote>) {
    let mut ir = CadIr::empty();
    let mut refusal = LaneRefusals::new();
    crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut AnnotationBuilder::new(),
            patches,
            &mut refusal,
            &mut FamilyEntityAdmission::new(ctx),
            &BodyRoots::new(),
        )
    })
    .expect("transfer generated sheets");
    ir.finalize();
    (ir, refusal.take_notes())
}

fn assert_reconciliation(notes: &[LossNote], expected: [usize; 4], max_gap_mm: f64) {
    let notes = notes
        .iter()
        .filter(|note| note.code.local_code() == "topology.shared-boundary-reconciled")
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one shared-boundary reconciliation note");
    };
    for (label, count) in ["vertex repair", "seed alignment", "shared-edge retry"]
        .into_iter()
        .zip(expected)
    {
        assert!(
            note.message.contains(&format!("{label} on {count} faces")),
            "{}",
            note.message
        );
    }
    assert!(
        note.message.contains(&format!(
            "Replaced shared curves on {} previously admitted faces",
            expected[3]
        )),
        "{}",
        note.message
    );
    assert!(
        note.message
            .contains(&format!("Largest discrepancy was {max_gap_mm:.9} mm")),
        "{}",
        note.message
    );
}

fn assert_no_reconciliation(notes: &[LossNote]) {
    assert!(notes
        .iter()
        .all(|note| note.code.local_code() != "topology.shared-boundary-reconciled"));
}

pub(super) fn assert_shared_sheet(ir: &CadIr) {
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate recovered sheet");
    assert!(validation.is_ok(), "{:?}", validation.findings);
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.edges.len(), 7);
    assert_eq!(ir.model.coedges.len(), 8);
    for face in &ir.model.faces {
        assert_outer_winding(ir, face);
    }
    let shared = ir
        .model
        .coedges
        .iter()
        .filter(|side| side.radial_next != side.id)
        .collect::<Vec<_>>();
    let [first, second] = shared.as_slice() else {
        panic!("one shared edge");
    };
    assert_eq!(first.edge, second.edge);
    assert_ne!(first.sense, second.sense);
    assert_eq!(first.radial_next, second.id);
    assert_eq!(second.radial_next, first.id);
}

fn bow_shared_edge(patch: &mut Patch, offset: f64) {
    let side = &mut patch.sides[3];
    let points = [
        Point2::new(1.0, 0.0),
        Point2::new(1.0 + offset, 0.5),
        Point2::new(1.0, 1.0),
    ];
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    side.curve = NurbsCurve::from_lanes(
        2,
        knots.clone(),
        points
            .map(|point| Point3::new(point.u, point.v, 0.0))
            .to_vec(),
        None,
        false,
    )
    .expect("bowed boundary curve");
    side.pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(2, knots, points.to_vec(), None, false)
            .expect("bowed boundary pcurve"),
    };
}

#[test]
fn shared_sheet_repairs_a_vertex_corroborated_by_another_edge() {
    let mut patches = two_face_sheet();
    patches[1].sides[0].vertices[0].1.x += 0.003;
    let (ir, notes) = transfer_with_notes(patches);
    assert_shared_sheet(&ir);
    assert_reconciliation(&notes, [1, 0, 0, 0], 0.003);
}

#[test]
fn shared_sheet_retries_after_a_later_neighbor_is_admitted() {
    let mut patches = two_face_sheet();
    patches[1].sides[0].vertices[0].1.x += 0.003;
    patches.reverse();
    let (ir, notes) = transfer_with_notes(patches);
    assert_shared_sheet(&ir);
    assert_reconciliation(&notes, [1, 0, 1, 0], 0.003);
}

#[test]
fn shared_sheet_requires_corroboration_and_bounds_vertex_repairs() {
    let mut uncorroborated = two_face_sheet();
    uncorroborated[1].sides[0].vertices[0].1.x += 0.003;
    uncorroborated[1].sides[3].vertices[0].1.x += 0.003;
    assert_eq!(transfer(uncorroborated).model.faces.len(), 1);

    let mut excessive = two_face_sheet();
    excessive[1].sides[0].vertices[0].1.x += 0.006;
    assert_eq!(transfer(excessive).model.faces.len(), 1);
}

#[test]
fn shared_sheet_reprojects_a_bounded_boundary_discrepancy() {
    let mut patches = two_face_sheet();
    bow_shared_edge(&mut patches[1], 0.004);
    let (ir, notes) = transfer_with_notes(patches);
    assert_shared_sheet(&ir);
    assert_reconciliation(&notes, [0, 0, 1, 0], 0.002);
}

#[test]
fn shared_sheet_rejects_an_excessive_boundary_discrepancy() {
    let mut patches = two_face_sheet();
    bow_shared_edge(&mut patches[1], 0.02);
    assert_eq!(transfer(patches).model.faces.len(), 1);
}

#[test]
fn shared_sheet_reprojection_keeps_the_surface_lift_limit() {
    let mut patches = two_face_sheet();
    let patch = &mut patches[1];
    let gap = 0.002;
    patch.surface = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, gap),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("parallel plane"),
    );
    for side in &mut patch.sides {
        for (_, point) in &mut side.vertices {
            point.z += gap;
        }
        side.curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            side.vertices.map(|(_, point)| point).to_vec(),
            None,
            false,
        )
        .expect("parallel plane boundary");
    }
    assert_eq!(transfer(patches).model.faces.len(), 1);
}

#[test]
fn endpoint_witnesses_require_a_distinct_face_and_edge() {
    let patches = two_face_sheet();
    let mut side = patches[1].sides[3].clone();
    for (_, point) in &mut side.vertices {
        point.x += 0.003;
    }
    let witnesses = endpoint_witnesses(&patches[..1]);
    assert!(has_trim_witnesses(patches[1].face, &side, &witnesses));

    let same_edge = patches[0].sides[1].clone();
    let mut single = two_face_sheet().remove(0);
    single.sides = vec![same_edge];
    let witnesses = endpoint_witnesses(&[single]);
    assert!(!has_trim_witnesses(patches[1].face, &side, &witnesses));
    assert!(!has_trim_witnesses(
        patches[0].face,
        &side,
        &endpoint_witnesses(&patches[..1])
    ));
}

#[test]
fn shared_sheet_recovery_preserves_hole_containment() {
    let mut patches = two_face_sheet();
    let patch = &mut patches[1];
    bow_shared_edge(patch, -0.004);
    let corners = [[1.0005, 0.4], [0.9995, 0.4], [0.9995, 0.6], [1.0005, 0.6]];
    patch.inner_starts.push(4);
    for index in 0..4 {
        let next = (index + 1) % 4;
        let uv = [corners[index], corners[next]];
        let points = uv.map(|[x, y]| Point3::new(x, y, 0.0));
        patch.sides.push(Use {
            edge: 10 + index,
            vertices: [(10 + index, points[0]), (10 + next, points[1])],
            curve: NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                points.to_vec(),
                None,
                false,
            )
            .expect("hole edge"),
            pcurve: line_pcurve([0.0, 1.0], uv).expect("hole pcurve"),
            forward: true,
            inferred_accuracy: None,
            lift_error_mm: 0.0,
        });
    }
    let (ir, notes) = transfer_with_notes(patches);
    assert_eq!(ir.model.faces.len(), 1);
    assert_no_reconciliation(&notes);
}

#[test]
fn shared_sheet_reports_seed_admission_and_replaced_prior_curve() {
    let mut patches = two_face_sheet();
    let side = &mut patches[0].sides[1];
    let uv = [[1.0, 0.0], [1.0, 1.003]];
    side.curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        uv.map(|[x, y]| Point3::new(x, y, 0.0)).to_vec(),
        None,
        false,
    )
    .expect("overextended boundary");
    side.pcurve = line_pcurve([0.0, 1.0], uv).expect("overextended pcurve");
    let (ir, notes) = transfer_with_notes(patches);
    assert_shared_sheet(&ir);
    assert_reconciliation(&notes, [0, 1, 0, 1], 0.003);
    let shared = ir
        .model
        .edges
        .iter()
        .find(|edge| {
            ir.model
                .coedges
                .iter()
                .filter(|side| side.edge == edge.id)
                .count()
                == 2
        })
        .expect("shared edge");
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == shared.carrier.curve())
        .expect("shared curve");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &curve.geometry else {
        panic!("fitted shared curve");
    };
    assert!(
        curve_point(curve, 1.0)
            .expect("reconciled end")
            .distance(Point3::new(1.0, 1.0, 0.0))
            <= GAP_MM
    );
}

#[test]
fn shared_sheet_omits_reconciliation_for_an_ordinary_transfer() {
    let (ir, notes) = transfer_with_notes(two_face_sheet());
    assert_shared_sheet(&ir);
    assert_no_reconciliation(&notes);
}

#[test]
fn shared_sheet_omits_reconciliation_for_a_rejected_repair() {
    let mut patches = two_face_sheet();
    patches[1].sides[0].vertices[0].1.x += 0.003;
    patches[1].sides[1].vertices[0].1.x += 0.02;
    let (ir, notes) = transfer_with_notes(patches);
    assert_eq!(ir.model.faces.len(), 1);
    assert_no_reconciliation(&notes);
}

#[test]
fn shared_sheet_aggregates_only_emitted_reconciliations() {
    let mut patches = two_face_sheet();
    patches[1].sides[0].vertices[0].1.x += 0.003;
    let mut other = two_face_sheet();
    for patch in &mut other {
        patch.face += 100;
        patch.body = (0, 20);
    }
    other[1].sides[0].vertices[0].1.x += 0.004;
    patches.extend(other);
    let (ir, notes) = transfer_with_notes(patches);
    assert_eq!(ir.model.faces.len(), 4);
    assert_reconciliation(&notes, [2, 0, 0, 0], 0.004);
}
