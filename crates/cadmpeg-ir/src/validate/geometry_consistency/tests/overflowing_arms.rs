// SPDX-License-Identifier: Apache-2.0
//! Validator findings where an evaluator arm reports a non-finite value.

use super::{check_procedural_support_consistency, mapped_surface_curve_with_pcurve};
use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::math::{Point2, Point3, Vector3};

#[test]
fn a_support_side_whose_nurbs_pcurve_overflows_misses_its_contract_by_nan() {
    // The linear NURBS pcurve through (0, 0) and (f64::MAX, 0) extrapolates
    // past its knot interval: at 2 and 3 its u coordinate is twice and three
    // times f64::MAX. The plane maps the infinite u to a point with no finite
    // coordinate.
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(f64::MAX, 0.0)],
            None,
            false,
        )
        .unwrap(),
    };
    let ir = mapped_surface_curve_with_pcurve(pcurve, [2.0, 3.0]);
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].message,
        "procedural support side 0 misses its endpoint distance contract by NaN"
    );
}

#[test]
fn a_support_side_whose_placed_support_overflows_misses_its_contract_by_inf() {
    // The pcurve maps the range to u = 1e300 and u = 2e300; the placement
    // adds f64::MAX to x, which leaves the finite range at both ends.
    let mut ir = super::mapped_surface_curve([1.0e300, 2.0e300]);
    let plane = ir.model.surfaces[0].geometry.clone();
    let crate::geometry::SurfaceGeometry::Solved(plane) = plane else {
        panic!("the fixture support is solved");
    };
    ir.model.surfaces[0].geometry = crate::geometry::SurfaceGeometry::Solved(
        crate::geometry::SolvedSurfaceGeometry::Transformed(
            crate::geometry::PlacedSurface::try_new(
                Box::new(plane),
                crate::transform::Transform::affine([
                    [1.0, 0.0, 0.0, f64::MAX],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .unwrap(),
            )
            .unwrap(),
        ),
    );
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].message,
        "procedural support side 0 misses its endpoint distance contract by inf"
    );
}

#[test]
fn a_coedge_placed_pcurve_whose_mapped_points_overflow_misses_the_vertices_by_nan() {
    // The vertical line at u = MAX, placed by a transform that doubles u,
    // reaches u = +inf at every parameter, and the face plane maps it to a
    // point with no finite coordinate.
    let mut ir = crate::examples::unit_cube().expect("valid unit cube fixture");
    let id = crate::ids::PcurveId::mint("synthetic:cube:pcurve#placed-overflow")
        .expect("valid identity");
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Transformed(
            crate::geometry::pcurve::PlacedPcurve::try_new(
                Box::new(PcurveGeometry::Line(
                    crate::geometry::pcurve::LinePcurve::try_new(
                        Point2::new(f64::MAX, 0.0),
                        Point2::new(0.0, 1.0),
                    )
                    .unwrap(),
                )),
                crate::transform::Transform2::affine([[2.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).unwrap(),
            )
            .unwrap(),
        ),
        metadata: crate::geometry::pcurve::PcurveMetadata::default(),
    });
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| {
            coedge.id.as_str().contains("bottom") && coedge.edge.as_str() == "synthetic:cube:edge#0"
        })
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: None,
    }];
    let coedge_id = coedge.id.as_str().to_owned();
    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(
        findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(coedge_id.as_str())
                && finding.message
                    == "pcurve mapped through the face surface misses the edge's vertex positions \
                        by NaN"
        }),
        "{findings:?}"
    );
}

/// The parabola with vertex `vertex` along x, opening in y, with focal
/// distance `focal`: its point at `t` is `vertex + (focal t^2, 2 focal t, 0)`.
fn parabola(vertex: Point3, focal: f64) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Parabola(
        crate::geometry::analytic::ParabolaCurve::try_new(
            vertex,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            focal,
        )
        .unwrap(),
    )
}

/// Every finding's message, in order.
fn messages(findings: &[crate::report::check::Finding]) -> Vec<&str> {
    findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect()
}

#[test]
fn a_surface_curve_whose_end_overflows_misses_its_support_contract_by_nan() {
    // The solved parabola through (2, 0, 0) at t = 0 has the coefficient
    // 2 MAX t at t = 1, which overflows: its end reaches no coordinate, and
    // the NaN mismatch there is the finding's measure.
    let mut ir = super::mapped_surface_curve([2.0, 3.0]);
    *ir.model.curves[0]
        .geometry
        .solved_cache_mut()
        .expect("mapped curve has a solved cache") = parabola(Point3::new(2.0, 0.0, 0.0), f64::MAX);
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(
        messages(&findings),
        ["procedural support side 0 misses its endpoint distance contract by NaN"]
    );
}

#[test]
fn a_surface_offset_whose_solved_end_overflows_misses_its_offset_distance_by_nan() {
    let mut ir = super::mapped_surface_offset();
    *ir.model.curves[0]
        .geometry
        .solved_cache_mut()
        .expect("mapped curve has a solved cache") =
        parabola(Point3::new(2.0, 0.0, 25.0), f64::MAX);
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(
        messages(&findings),
        ["surface-offset solved curve misses its base offset distance by NaN"]
    );
}

#[test]
fn a_surface_offset_whose_base_end_overflows_misses_its_contracts_by_nan() {
    // The base parabola with focal distance MAX / 8 about (-MAX / 2,
    // -MAX / 2, 0) reaches the origin at t = 2; at t = 3 its coefficient
    // 9 MAX / 8 overflows.
    let mut ir = super::mapped_surface_offset();
    ir.model.curves[1].geometry = CurveGeometry::Solved(parabola(
        Point3::new(-f64::MAX / 2.0, -f64::MAX / 2.0, 0.0),
        f64::MAX / 8.0,
    ));
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(
        messages(&findings),
        [
            "surface-offset solved curve misses its base offset distance by NaN",
            "procedural support side 0 misses its endpoint distance contract by NaN",
        ]
    );
}

#[test]
fn a_charted_tolerant_intersection_whose_end_overflows_misses_its_witnesses_by_nan() {
    // The line pcurve u = MAX (1 + t) reaches u = 0 at t = -1 and u = +inf
    // at t = 1, where neither plane lifts a finite point.
    let mut ir = crate::document::CadIr::empty();
    let plane = |normal: Vector3| {
        crate::geometry::SurfaceGeometry::Solved(crate::geometry::SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                normal,
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ))
    };
    let supports = ["test:model:surface#first", "test:model:surface#second"]
        .map(|id| crate::ids::SurfaceId::mint(id).unwrap());
    ir.model.surfaces.extend([
        crate::geometry::Surface {
            id: supports[0].clone(),
            geometry: plane(Vector3::new(0.0, 0.0, 1.0)),
            source_object: None,
        },
        crate::geometry::Surface {
            id: supports[1].clone(),
            geometry: plane(Vector3::new(0.0, -1.0, 0.0)),
            source_object: None,
        },
    ]);
    let curve = crate::ids::CurveId::mint("test:model:curve#intersection").unwrap();
    ir.model.curves.push(crate::geometry::Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Line(
        crate::geometry::pcurve::LinePcurve::try_new(
            Point2::new(f64::MAX, 0.0),
            Point2::new(f64::MAX, 0.0),
        )
        .unwrap(),
    );
    ir.model
        .add_procedural_curve(
            curve,
            crate::geometry::ProceduralCurve::new(
                crate::ids::ProceduralCurveId::mint("test:model:procedural#intersection").unwrap(),
                crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
                    construction: crate::geometry::TolerantIntersectionConstruction::try_new(
                        supports,
                        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                        1.0e-9,
                    )
                    .unwrap(),
                    parameterization: Some(
                        crate::geometry::TolerantIntersectionParameterization::try_new(
                            [pcurve.clone(), pcurve],
                            [-1.0, 1.0],
                        )
                        .unwrap(),
                    ),
                    cache: None,
                },
            ),
        )
        .unwrap();
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(
        messages(&findings),
        ["charted tolerant intersection misses its endpoint witnesses by NaN"]
    );
}

/// The unit cube whose first edge's curve is the parabola through the edge's
/// start vertex at t = 0 with focal distance MAX / 64: at the edge's end
/// parameter 10 its coefficient 100 MAX / 64 overflows.
fn cube_with_an_overflowing_edge_curve() -> crate::document::CadIr {
    let mut ir = crate::examples::unit_cube().expect("valid unit cube fixture");
    let edge = ir.model.edges[0].clone();
    let start = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .and_then(|vertex| {
            ir.model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
        })
        .expect("edge start point")
        .position()
        .get();
    let curve = edge.curve().expect("cube edge curve").clone();
    ir.model
        .curves
        .iter_mut()
        .find(|candidate| candidate.id == curve)
        .expect("cube edge curve")
        .geometry = CurveGeometry::Solved(parabola(start, f64::MAX / 64.0));
    ir
}

#[test]
fn an_edge_curve_whose_end_overflows_misses_its_vertices_by_nan() {
    let ir = cube_with_an_overflowing_edge_curve();
    let edge = ir.model.edges[0].id.as_str().to_owned();
    let mut findings = Vec::new();
    super::super::check_edge_endpoint_consistency(&ir, &mut findings);
    assert!(
        findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(edge.as_str())
                && finding.message == "edge curve endpoints miss the edge's vertex positions by NaN"
        }),
        "{findings:?}"
    );
}

#[test]
fn a_coedge_use_curve_whose_end_overflows_misses_its_traversal_vertices_by_nan() {
    let mut ir = cube_with_an_overflowing_edge_curve();
    let edge = ir.model.edges[0].clone();
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge.id)
        .expect("a coedge uses the first edge");
    coedge.use_curve = Some(crate::topology::CoedgeUseCurve {
        curve: edge.curve().expect("cube edge curve").clone(),
        parameter_range: crate::topology::ParameterInterval::new([0.0, 10.0]).unwrap(),
    });
    let coedge = coedge.id.as_str().to_owned();
    let mut findings = Vec::new();
    super::super::check_edge_endpoint_consistency(&ir, &mut findings);
    assert!(
        findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(coedge.as_str())
                && finding.message
                    == "coedge use-curve endpoints miss the traversal vertices by NaN"
        }),
        "{findings:?}"
    );
}
