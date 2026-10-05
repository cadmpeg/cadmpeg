// SPDX-License-Identifier: Apache-2.0
//! Mutable binding lookup admission.

use super::super::bind_consolidated_revolution_faces_and_seams;
use crate::families::freeform::ConsolidatedRevolutionBinding;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
    ProceduralCurveDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};
use cadmpeg_ir::AnnotationBuilder;

fn fixture() -> (CadIr, ConsolidatedRevolutionBinding) {
    let mut ir = CadIr::empty();
    let surface_ids = [
        SurfaceId::mint("catia:test:surface#face-surface%230".to_string())
            .expect("identity grammar"),
        SurfaceId::mint("catia:test:surface#face-surface%231".to_string())
            .expect("identity grammar"),
    ];
    for id in &surface_ids {
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
    }
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            3.0,
        )
        .expect("valid TorusSurface fixture"),
    ));
    let profile_end = std::f64::consts::PI - 0.5;
    let positions = [
        Point3::new(-1.0, 0.0, 0.0),
        Point3::new(2.0 + 3.0 * profile_end.cos(), 0.0, 3.0 * profile_end.sin()),
    ];
    for (index, position) in positions.into_iter().enumerate() {
        let point =
            PointId::mint(format!("catia:test:point#point%23{index}")).expect("identity grammar");
        ir.model.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: VertexId::mint(format!("catia:test:vertex#vertex%23{index}"))
                .expect("identity grammar"),
            point,
            tolerance: None,
        });
    }
    let curve_id =
        CurveId::mint("catia:test:curve#seam-curve".to_string()).expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId::mint("catia:test:edge#seam-edge".to_string()).expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id.clone()), Some([0.0, 1.0]))
            .expect("valid edge carrier"),
        start: VertexId::mint("catia:test:vertex#vertex%230".to_string())
            .expect("identity grammar"),
        end: VertexId::mint("catia:test:vertex#vertex%231".to_string()).expect("identity grammar"),
        tolerance: None,
    });
    for (side, surface) in surface_ids.iter().enumerate() {
        let face =
            FaceId::mint(format!("catia:test:face#face%23{side}")).expect("identity grammar");
        let loop_id =
            LoopId::mint(format!("catia:test:loop#loop%23{side}")).expect("identity grammar");
        let coedge =
            CoedgeId::mint(format!("catia:test:coedge#coedge%23{side}")).expect("identity grammar");
        ir.model.faces.push(Face {
            id: face.clone(),
            shell: ShellId::mint("catia:test:shell#shell".to_string()).expect("identity grammar"),
            surface: surface.clone(),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face,
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(
                    &cadmpeg_test_support::service_decode_context(),
                    vec![coedge.clone()],
                    Vec::new(),
                )
                .expect("fixture ring admission")
                .expect("valid loop ring"),
            ),
        });
        ir.model.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id,
            edge: EdgeId::mint("catia:test:edge#seam-edge".to_string()).expect("identity grammar"),
            radial_next: CoedgeId::mint(format!("catia:test:coedge#coedge%23{}", 1 - side))
                .expect("identity grammar"),
            sense: if side == 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    ir.model
        .add_procedural_curve(
            &cadmpeg_ir::document::admission::StandardAdmission,
            &curve_id,
            ProceduralCurve::new(
                ProceduralCurveId::mint("catia:test:proceduralcurve#seam-construction".to_string())
                    .expect("identity grammar"),
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::try_new(
                        std::array::from_fn(|side| IntcurveSupportSide {
                            surface: Some(surface_ids[side].clone()),
                            pcurve: None,
                        }),
                        [0.0, 1.0],
                        std::array::from_fn(|_| Vec::new()),
                    )
                    .expect("valid IntcurveSupportContext fixture"),
                    discontinuity_flag: false,
                    cache: None,
                },
            ),
        )
        .expect("procedural curve admission")
        .expect("attach construction to its fixture carrier");
    (
        ir,
        ConsolidatedRevolutionBinding {
            geometry,
            profile_sweep: 0.5,
        },
    )
}

fn assert_lookup_refusal(operation: &'static str) {
    let (mut ir, binding) = fixture();
    let expected = binding.geometry.clone();
    let outcome = crate::test_support::with_service_context(|ctx| {
        bind_consolidated_revolution_faces_and_seams(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            std::slice::from_ref(&binding),
        )
    })
    .expect("service admits the torus bindings and meridian seam");
    assert_eq!(outcome, (2, 1));
    assert!(ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.geometry == expected));
    assert!(matches!(
        ir.model.curves[0].geometry.solved_cache(),
        Some(SolvedCurveGeometry::Circle(circle_curve)) if circle_curve.radius().get() == 3.0
    ));
    assert_eq!(
        ir.model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 0.5]),
    );

    let result = crate::test_support::with_work_refusal(operation, |ctx| {
        let (mut ir, binding) = fixture();
        let result = bind_consolidated_revolution_faces_and_seams(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            std::slice::from_ref(&binding),
        );
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::WorkUnits
            );
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == operation)
    );
}

#[test]
fn revolution_surface_binding_lookup_propagates_caller_work_refusal() {
    assert_lookup_refusal("catia_revolution_surface_binding_lookup");
}

#[test]
fn revolution_procedural_binding_lookup_propagates_caller_work_refusal() {
    assert_lookup_refusal("catia_revolution_procedural_binding_lookup");
}

#[test]
fn revolution_curve_edge_count_lookup_propagates_caller_work_refusal() {
    assert_lookup_refusal("catia_revolution_curve_edge_count_lookup");
}
