// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::math::Point2;

fn transfer(patches: Vec<Patch>) -> CadIr {
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut AnnotationBuilder::new(),
            patches,
            &mut LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("transfer generated sheets");
    ir.finalize();
    ir
}

fn assert_shared_sheet(ir: &CadIr) {
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
    assert_shared_sheet(&transfer(patches));
}

#[test]
fn shared_sheet_retries_after_a_later_neighbor_is_admitted() {
    let mut patches = two_face_sheet();
    patches[1].sides[0].vertices[0].1.x += 0.003;
    patches.reverse();
    assert_shared_sheet(&transfer(patches));
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
    assert_shared_sheet(&transfer(patches));
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
        });
    }
    assert_eq!(transfer(patches).model.faces.len(), 1);
}
