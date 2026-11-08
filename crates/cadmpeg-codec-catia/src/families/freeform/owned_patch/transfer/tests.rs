// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::families::freeform::owned_patch::line_pcurve;
use cadmpeg_ir::geometry::{analytic::PlaneSurface, nurbs::NurbsCurve, SolvedSurfaceGeometry};
use cadmpeg_ir::math::Vector3;

fn two_face_sheet() -> Vec<Patch> {
    let vertices = [
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [2.0, 0.0],
        [2.0, 1.0],
    ];
    let plane = PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("fixture plane");
    let edge = |edge, a, b, forward| {
        let uv: [[f64; 2]; 2] = [vertices[a], vertices[b]];
        let points = uv.map(|[x, y]| Point3::new(x, y, 0.0));
        Use {
            edge,
            vertices: [(a, points[0]), (b, points[1])],
            curve: NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                points.to_vec(),
                None,
                false,
            )
            .expect("linear fixture edge"),
            pcurve: line_pcurve([0.0, 1.0], uv).expect("linear fixture pcurve"),
            forward,
        }
    };
    let patch = |face, forward, sides| Patch {
        face,
        body: (0, 10),
        forward,
        surface: SolvedSurfaceGeometry::Plane(plane),
        priority: 0,
        sides,
        inner_starts: Vec::new(),
    };
    vec![
        patch(
            20,
            true,
            vec![
                edge(0, 0, 1, true),
                edge(1, 1, 2, true),
                edge(2, 2, 3, true),
                edge(3, 3, 0, true),
            ],
        ),
        patch(
            30,
            false,
            vec![
                edge(4, 1, 4, true),
                edge(5, 4, 5, true),
                edge(6, 5, 2, true),
                edge(1, 1, 2, false),
            ],
        ),
    ]
}

#[test]
fn owned_sheet_emits_one_shared_edge_and_reports_partial_transfer() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let loss = crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut annotations,
            two_face_sheet(),
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("sheet transfer succeeds")
    .expect("sheet transfer reports partial recovery");

    assert_eq!(loss.code.local_code(), "topology.owned-patch-subset");
    assert!(loss.message.starts_with("Transferred 2 bounded faces"));
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.faces[0].sense, Sense::Forward);
    assert_eq!(ir.model.faces[1].sense, Sense::Reversed);
    assert_eq!(ir.model.edges.len(), 7);
    assert_eq!(ir.model.coedges.len(), 8);
    let shared: Vec<_> = ir
        .model
        .coedges
        .iter()
        .filter(|side| side.radial_next != side.id)
        .collect();
    assert_eq!(shared.len(), 2);
    assert_eq!(shared[0].edge, shared[1].edge);
    assert_ne!(shared[0].owner_loop, shared[1].owner_loop);
    assert_eq!(shared[0].radial_next, shared[1].id);
    assert_eq!(shared[1].radial_next, shared[0].id);
    assert_eq!(shared[0].sense, Sense::Forward);
    assert_eq!(shared[1].sense, Sense::Reversed);
}
