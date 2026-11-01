// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::geometry::nurbs::{NurbsPoleGrid, NurbsSurfaceAxis};
use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
use crate::geometry::FitTolerance;
use crate::topology::{CoedgeUseCurve, ParameterInterval};

fn support_curve(miss: f64, fit: Option<f64>) -> CadIr {
    support_curve_with_heights([miss, 0.0], fit)
}

/// A support pcurve at `ends` over the construction ends and `middle` over
/// the interior of the interval.
fn support_curve_with_heights([ends, middle]: [f64; 2], fit: Option<f64>) -> CadIr {
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            1,
            vec![0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
            PcurveNurbsPoles::Polynomial {
                points: vec![
                    Point2::new(2.0, ends),
                    Point2::new(2.25, middle),
                    Point2::new(2.75, middle),
                    Point2::new(3.0, ends),
                ],
            },
            false,
        )
        .unwrap(),
    };
    let mut ir = mapped_surface_curve_with_pcurve(pcurve, [0.0, 1.0]);
    let mut surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 4.0, 4.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 4.0, 4.0], false),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 4.0, 0.0)],
                vec![Point3::new(4.0, 0.0, 0.0), Point3::new(4.0, 4.0, 0.0)],
            ],
        },
        false,
    )
    .unwrap();
    surface.set_cache_fit_tolerance(fit.map(|value| FitTolerance::try_new(value).unwrap()));
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    ir
}

fn using_edge(ir: &mut CadIr, range: [f64; 2]) {
    let mut cube = unit_cube().unwrap();
    let mut edge = cube.model.edges.remove(0);
    edge.set_curve(Some(ir.model.curves[0].id.clone())).unwrap();
    edge.set_param_range(Some(ParameterInterval::new(range).unwrap()));
    ir.model.edges.push(edge);
}

fn assert_support_failure(ir: &CadIr, fails: bool) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = crate::index::ModelIndex::new_model_only(ir);
    let mut findings = Vec::new();
    check_procedural_support_consistency(
        &ctx,
        &index,
        &crate::eval::admission::ModelEvaluationPass::new(),
        &mut findings,
    )
    .unwrap();
    assert_eq!(!findings.is_empty(), fails, "{findings:?}");
    let context = match ir.model.procedural_curves[0].definition() {
        ProceduralCurveDefinition::SurfaceCurve { family } => family.context(),
        ProceduralCurveDefinition::Intersection { context, .. } => context,
        _ => panic!("support context"),
    };
    let sides = super::super::cache_support_endpoint_failures(
        &ctx,
        &index,
        &ir.model.curves[0],
        context,
        ir.model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(FitTolerance::get),
    )
    .unwrap();
    assert_eq!(sides, if fails { vec![0] } else { Vec::new() });
}

#[test]
fn support_fit_bounds_the_endpoint_miss() {
    assert_support_failure(&support_curve(0.05, Some(0.1)), false);
}

#[test]
fn endpoint_miss_above_support_fit_is_reported() {
    assert_support_failure(&support_curve(0.2, Some(0.1)), true);
}

#[test]
fn construction_end_miss_outside_used_hull_passes() {
    let mut ir = support_curve(0.2, None);
    using_edge(&mut ir, [0.25, 0.75]);
    assert_support_failure(&ir, false);
}

#[test]
fn used_hull_endpoint_miss_above_allowance_is_reported() {
    let mut ir = support_curve(0.2, None);
    using_edge(&mut ir, [0.125, 0.75]);
    assert_support_failure(&ir, true);
}

#[test]
fn unused_curve_checks_the_full_construction_interval() {
    assert_support_failure(&support_curve(0.2, None), true);
}

#[test]
fn all_edges_and_coedge_use_ranges_contribute_to_the_hull() {
    let mut ir = support_curve(0.2, None);
    using_edge(&mut ir, [0.25, 0.5]);
    using_edge(&mut ir, [0.5, 0.75]);
    assert_support_failure(&ir, false);
    let mut coedge = unit_cube().unwrap().model.coedges.remove(0);
    coedge.use_curve = Some(CoedgeUseCurve {
        curve: ir.model.curves[0].id.clone(),
        parameter_range: ParameterInterval::new([0.125, 0.5]).unwrap(),
    });
    ir.model.coedges.push(coedge);
    assert_support_failure(&ir, true);
}

#[test]
fn used_hull_keeps_the_context_pcurve_parameter_map() {
    let mut ir = mapped_surface_curve([2.0, 3.0]);
    using_edge(&mut ir, [0.25, 0.75]);
    assert_support_failure(&ir, false);
}

/// A curve that runs against its construction parameter, as a reversed
/// record does: the cache at `t` is the construction at `-t`.
fn reversed_support_curve(miss: f64) -> CadIr {
    let mut ir = support_curve(miss, None);
    *ir.model.curves[0]
        .geometry
        .solved_cache_mut()
        .expect("solved cache") = SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(2.0, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
        )
        .unwrap(),
    );
    ir
}

#[test]
fn reversed_curve_reads_its_support_at_the_negated_hull() {
    let mut ir = reversed_support_curve(0.2);
    using_edge(&mut ir, [-0.75, -0.25]);
    assert_support_failure(&ir, false);
}

#[test]
fn reversed_curve_reports_a_miss_inside_its_hull() {
    let mut ir = reversed_support_curve(0.2);
    using_edge(&mut ir, [-0.875, -0.25]);
    assert_support_failure(&ir, true);
}

#[test]
fn construction_ends_answer_an_interior_hull_miss() {
    let mut ir = support_curve_with_heights([0.0, 0.2], None);
    using_edge(&mut ir, [0.25, 0.75]);
    assert_support_failure(&ir, false);
}

#[test]
fn hull_on_neither_interval_keeps_the_full_construction_interval() {
    let mut ir = support_curve(0.2, None);
    using_edge(&mut ir, [5.0, 6.0]);
    assert_support_failure(&ir, true);
}

#[test]
fn an_unranged_edge_keeps_the_full_construction_interval() {
    let mut ir = support_curve(0.2, None);
    using_edge(&mut ir, [0.25, 0.75]);
    using_edge(&mut ir, [0.25, 0.75]);
    ir.model.edges[1].set_param_range(None);
    assert_support_failure(&ir, true);
}

#[test]
fn typed_support_definition_supplies_the_cache_fit() {
    let mut ir = support_curve(0.05, None);
    let surface = ir.model.surfaces[0].id.clone();
    let definition = procedural_surface! {
        id: ProceduralSurfaceId::mint("test:model:procedural-surface#fit").unwrap(),
        definition: ProceduralSurfaceDefinition::Exact(
            crate::geometry::surface_payloads::ExactSurfacePayload::try_new(crate::geometry::ExactSpline::Legacy {
                ranges: [[0.0, 4.0], [0.0, 4.0]],
                extension: 0,
                cache: None,
            }).unwrap()
        ),
        cache_fit_tolerance: Some(0.1),
        record_bounds: None,
    };
    ir.model
        .add_procedural_surface(&surface, definition)
        .unwrap();
    assert_support_failure(&ir, false);
}

#[test]
fn curve_fit_above_the_support_fit_sets_the_allowance() {
    let mut ir = support_curve(0.2, Some(0.1));
    let ProceduralCurveDefinition::SurfaceCurve { family } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("surface curve");
    };
    let context = family.context().clone();
    ir.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag: false,
        cache: None,
    });
    ir.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.3))
        .unwrap();
    assert_support_failure(&ir, false);
}

fn offset_with_reparameterized_base(height: f64) -> CadIr {
    let mut ir = mapped_surface_offset();
    let ProceduralCurveDefinition::SurfaceOffset(payload) =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("offset");
    };
    let context = payload.context().clone();
    let base = payload.base().clone();
    ir.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::SurfaceOffset(
        crate::geometry::curve_payloads::SurfaceOffsetCurveConstruction::try_new(
            context,
            false,
            [[0.0, 1.0], [0.0, 1.0]],
            (base, [0.0, 1.0], [Some(0.0), Some(1.0)]),
            crate::geometry::CacheContract::from_form(None),
            25.0,
            [0.0, 1.0],
        )
        .unwrap(),
    ));
    ir.model.curves[1].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        crate::geometry::nurbs::NurbsCurve::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(3.0, 0.0, 0.0),
            ],
            None,
            false,
        )
        .unwrap(),
    ));
    *ir.model.curves[0].geometry.solved_cache_mut().unwrap() = SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(2.0, 0.0, height),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    );
    using_edge(&mut ir, [0.25, 0.75]);
    ir
}

#[test]
fn offset_used_hull_keeps_base_endpoint_witnesses_on_their_stored_parameters() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = offset_with_reparameterized_base(25.0);
    let index = crate::index::ModelIndex::new_model_only(&ir);
    let mut findings = Vec::new();
    check_procedural_support_consistency(
        &ctx,
        &index,
        &crate::eval::admission::ModelEvaluationPass::new(),
        &mut findings,
    )
    .unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn offset_used_hull_above_its_distance_allowance_is_reported() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = offset_with_reparameterized_base(26.0);
    let index = crate::index::ModelIndex::new_model_only(&ir);
    let mut findings = Vec::new();
    check_procedural_support_consistency(
        &ctx,
        &index,
        &crate::eval::admission::ModelEvaluationPass::new(),
        &mut findings,
    )
    .unwrap();
    assert!(
        findings.iter().any(|finding| finding
            .message
            .starts_with("procedural support side 0 misses")),
        "{findings:?}"
    );
}
