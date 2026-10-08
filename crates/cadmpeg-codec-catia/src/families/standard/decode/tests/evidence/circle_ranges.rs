// SPDX-License-Identifier: Apache-2.0

use crate::families::standard::decode::edge_geometry::{
    build_standard_edge_curve, native_support_circle_param_range,
};
use crate::families::standard::decode::tests::checked_circle;
use crate::families::standard::decode::StandardEdgeSupport;
use crate::families::standard::records::StandardCurveSupport;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::HashMap;

#[test]
fn native_support_pcurve_midpoint_selects_an_unwitnessed_circle_branch() {
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cylinder.clone()),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cylinder),
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [0.0, 1.5 * std::f64::consts::PI],
    };
    let start = Point3::new(1.0, 0.0, 0.0);
    let end = Point3::new(0.0, -1.0, 0.0);
    assert_eq!(
        native_support_circle_param_range(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            [start, end]
        )
        .expect("evaluator allocation succeeds"),
        Some([0.0, 1.5 * std::f64::consts::PI])
    );
    let mut disagreeing = native.clone();
    disagreeing.pcurves[1] = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    assert!(native_support_circle_param_range(
        &cadmpeg_test_support::service_decode_context(),
        &disagreeing,
        Point3::new(0.0, 0.0, 0.0),
        1.0,
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        [start, end]
    )
    .expect("evaluator allocation succeeds")
    .is_none());
    assert!(native_support_circle_param_range(
        &cadmpeg_test_support::service_decode_context(),
        &native,
        Point3::new(0.0, 0.0, 0.0),
        1.0,
        Vector3::new(0.0, 0.0, -1.0),
        Vector3::new(1.0, 0.0, 0.0),
        [start, end]
    )
    .expect("evaluator allocation succeeds")
    .is_none());

    let mut ir = CadIr::empty();
    for (index, position) in [start, end].into_iter().enumerate() {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: checked_circle(Point3::new(0.0, 0.0, 0.0), 1.0),
    };
    let (_, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                native_surfaces:
                    &mut crate::families::standard::decode::edge_geometry::NativeSurfaceIndex::new(
                        admission.context(),
                    )
                    .expect("index reservation"),
                ir: &mut ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &[],
                surface_indices: &HashMap::new(),
                brep: &[],
                support: &support,
                points: [0, 1],
                native_support: Some(&native),
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    assert_eq!(range, Some([0.0, 1.5 * std::f64::consts::PI]));
}
