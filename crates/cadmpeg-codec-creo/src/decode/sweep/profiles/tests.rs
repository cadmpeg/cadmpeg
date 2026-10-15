// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement,
};

const EPS_WIDE_NURBS_AREA: f64 = 1.0e-12;
const EPS_QUARTER_CIRCLE_SEAM: f64 = 1.0e-12;

fn sketch(id: &SketchId, entity: &SketchEntityId) -> Sketch {
    Sketch {
        id: id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity.clone(),
            reversed: false,
        }]])
        .expect("valid test fixture"),
        native_ref: None,
    }
}

fn line_entity(id: &SketchEntityId, sketch: &SketchId, end: [f64; 2]) -> SketchEntity {
    SketchEntity::new(
        id.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(end[0], end[1]),
        })
        .expect("valid test fixture"),
    )
}

#[test]
fn borrowed_nurbs_profile_sampler_keeps_line_endpoints() {
    let nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("linear NURBS fixture");
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::nurbs_profile_polyline(ctx, &nurbs, 0.01))
            .expect("service profile resources"),
        Some(vec![[0.0, 0.0], [1.0, 0.0]])
    );
}

fn linear_profile_curve() -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("linear NURBS fixture")
}

fn linear_profile_polyline_with_policy(
    policy: cadmpeg_core::decode::DecodePolicy,
) -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
    let curve = linear_profile_curve();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    super::nurbs_profile_polyline(&ctx, &curve, 0.01)
}

#[test]
fn nurbs_profile_polyline_depth_refuses_before_recursive_span() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let error =
        linear_profile_polyline_with_policy(policy).expect_err("first span exceeds zero depth");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "creo NURBS profile sampling depth")
    );
    assert_eq!(
        linear_profile_polyline_with_policy(DecodePolicy::service()).expect("service polyline"),
        Some(vec![[0.0, 0.0], [1.0, 0.0]])
    );
}

fn profile_circle(center: [f64; 2], radius: f64, reversed: bool) -> super::ProfileEntity {
    super::ProfileEntity {
        geometry: super::ProfileGeometry::Circle {
            center: Point2::new(center[0], center[1]),
            radius: cadmpeg_ir::scalar::Length::new(radius).expect("positive radius"),
        },
        reversed,
        start: cadmpeg_ir::units::FinitePoint2::new(Point2::new(center[0] + radius, center[1]))
            .expect("finite"),
        end: cadmpeg_ir::units::FinitePoint2::new(Point2::new(center[0] + radius, center[1]))
            .expect("finite"),
    }
}

#[test]
fn hole_profiles_inside_outer_profile_are_ordered() {
    let profiles = vec![
        vec![profile_circle([0.0, 0.0], 5.0, false)],
        vec![profile_circle([-2.0, 0.0], 1.0, true)],
        vec![profile_circle([2.0, 0.0], 1.0, true)],
    ];
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::ordered_extrusion_profiles(ctx, profiles))
            .expect("service resources")
            .is_some()
    );
}

#[test]
fn connected_profile_vertices_read_line_endpoints() {
    let sketch_id = SketchId::mint("creo:model:sketch#71").expect("identity grammar");
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#71:1").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model
        .sketch_entities
        .push(line_entity(&entity_id, &sketch_id, [1.0, 0.0]));
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::connected_sketch_profile_vertices(ctx, &ir, &carriers, &sketch_id)
                .map(std::iter::Iterator::collect::<Vec<_>>)
        })
        .expect("service profile vertices"),
        vec![(0, vec![[0.0, 0.0], [1.0, 0.0]])]
    );
}

#[test]
fn source_sketch_geometry_drives_profile_analysis_after_millimeter_admission() {
    let sketch_id = SketchId::mint("creo:model:sketch#8").expect("identity grammar");
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#8:1").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::new(
        cadmpeg_ir::scalar::PositiveReal::new(25.4),
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch_entities(
            ctx,
            &mut ir,
            vec![SketchEntity::new(
                entity_id,
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(1.0, 0.0),
                    radius: cadmpeg_ir::scalar::Length::new(2.0).expect("finite source radius"),
                })
                .expect("source circle"),
            )],
        )
    })
    .expect("entity admission");
    let SketchGeometryDefinition::Circle { center, radius } =
        ir.model.sketch_entities[0].geometry.definition()
    else {
        panic!("admitted entity changed family");
    };
    assert_eq!(center.u, 25.4);
    assert_eq!(radius.get(), 50.8);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::connected_sketch_profile_vertices(
            ctx, &ir, &carriers, &sketch_id
        )
        .map(std::iter::Iterator::collect::<Vec<_>>))
        .expect("service profile vertices"),
        vec![(0, vec![[3.0, 0.0]])]
    );
    let profiles = crate::decode::with_test_decode_ctx(|ctx| {
        super::resolved_sketch_profiles(ctx, &ir, &carriers, &sketch_id, 1)
    })
    .expect("resource admission")
    .expect("source profile");
    let super::ProfileGeometry::Circle { center, radius } = profiles[0][0].geometry() else {
        panic!("source profile changed family");
    };
    assert_eq!(center.u, 1.0);
    assert_eq!(radius.get(), 2.0);
}

#[test]
fn resolved_nurbs_profile_keeps_closed_source_geometry() {
    let sketch_id = SketchId::mint("creo:model:sketch#74").expect("identity grammar");
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#74:1").expect("identity grammar");
    let nurbs = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("closed source NURBS");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id,
        sketch_id.clone(),
        SketchGeometry::nurbs(nurbs),
    ));
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let profiles = crate::decode::with_test_decode_ctx(|ctx| {
        super::resolved_sketch_profiles(ctx, &ir, &carriers, &sketch_id, 1)
    })
    .expect("service allocation")
    .expect("closed source profile");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(profiles[0][0].start(), profiles[0][0].end());
}

#[test]
fn resolved_profile_reads_closed_circle() {
    let sketch_id = SketchId::mint("creo:model:sketch#73").expect("identity grammar");
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#73:1").expect("identity grammar");
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(1.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(2.0).expect("finite radius"),
    })
    .expect("source circle");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model
        .sketch_entities
        .push(SketchEntity::new(entity_id, sketch_id.clone(), geometry));
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let profiles = crate::decode::with_test_decode_ctx(|ctx| {
        super::resolved_sketch_profiles(ctx, &ir, &carriers, &sketch_id, 1)
    })
    .expect("service allocation")
    .expect("closed circle profile");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(profiles[0][0].start(), profiles[0][0].end());
}

#[test]
fn forward_arc_sweep_reduces_a_wide_finite_angle_interval() {
    let sweep = super::forward_arc_sweep(-f64::MAX, f64::MAX);
    assert!(sweep.is_finite());
    assert!((0.0..std::f64::consts::TAU).contains(&sweep));
    assert!((sweep - 1.161_306_304_240_227_4).abs() <= f64::EPSILON);
}

#[test]
fn nurbs_profile_area_uses_finite_gauss_samples_on_a_wide_domain() {
    let geometry = SketchGeometry::nurbs(
        PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![-f64::MAX, -f64::MAX, f64::MAX, f64::MAX],
            vec![Point2::new(1.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .expect("wide finite sketch NURBS"),
    );
    let area = crate::decode::with_test_decode_ctx(|ctx| {
        super::nurbs_profile_signed_area_twice(ctx, &geometry, false)
    })
    .expect("service area resources")
    .expect("finite wide-domain area");
    assert!((area - 1.0).abs() <= EPS_WIDE_NURBS_AREA);
}

#[test]
fn profile_joins_reject_duplicate_sketch_ids() {
    let sketch_id = SketchId::mint("creo:model:sketch#7".to_string()).expect("valid test fixture");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#7:1".to_string())
        .expect("valid test fixture");
    let mut ir = CadIr::empty();
    ir.model.sketches.extend([
        sketch(&sketch_id, &entity_id),
        sketch(&sketch_id, &entity_id),
    ]);
    ir.model
        .sketch_entities
        .push(line_entity(&entity_id, &sketch_id, [1.0, 0.0]));

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::connected_sketch_profile_vertices(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id
        )
        .map(std::iter::Iterator::collect::<Vec<_>>))
        .expect("service profile vertices")
        .is_empty()
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::resolved_sketch_profiles(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id,
            1
        ))
        .expect("resource admission")
        .is_none()
    );
}

#[test]
fn profile_joins_reject_duplicate_sketch_entity_ids() {
    let sketch_id = SketchId::mint("creo:model:sketch#7".to_string()).expect("valid test fixture");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#7:1".to_string())
        .expect("valid test fixture");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model.sketch_entities.extend([
        line_entity(&entity_id, &sketch_id, [1.0, 0.0]),
        line_entity(&entity_id, &sketch_id, [0.0, 1.0]),
    ]);

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::connected_sketch_profile_vertices(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id
        )
        .map(std::iter::Iterator::collect::<Vec<_>>))
        .expect("service profile vertices")
        .is_empty()
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::resolved_sketch_profiles(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id,
            1
        ))
        .expect("resource admission")
        .is_none()
    );
}

#[test]
fn circular_pcurve_quarter_circle_spans_quarter_turn() {
    let pcurve = crate::decode::with_test_decode_ctx(|ctx| {
        super::circular_pcurve(
            ctx,
            [0.0, 0.0],
            1.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            &"quarter circle",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
    })
    .expect("service allocation")
    .expect("quarter-circle geometry");
    assert_eq!(
        cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &pcurve,
            0.0
        )
        .expect("start"),
        Point2::new(1.0, 0.0)
    );
    let end = cadmpeg_ir::eval::decode::pcurve_uv(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &pcurve,
        1.0,
    )
    .expect("end");
    assert!(end.u.abs() < EPS_QUARTER_CIRCLE_SEAM);
    assert!((end.v - 1.0).abs() < EPS_QUARTER_CIRCLE_SEAM);
}

#[test]
fn extrusion_cap_pcurve_wraps_an_arc_stored_beyond_one_turn() {
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, TAU};
    let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(1.0).expect("finite length fixture"),
        start_angle: cadmpeg_ir::scalar::Angle::new(0.0).expect("finite angle fixture"),
        end_angle: cadmpeg_ir::scalar::Angle::new(TAU + FRAC_PI_2).expect("finite angle fixture"),
    })
    .expect("valid test fixture");
    for (reversed, start, end) in [
        (false, [1.0, 0.0], [0.0, 1.0]),
        (true, [0.0, 1.0], [1.0, 0.0]),
    ] {
        let pcurve = crate::decode::with_test_decode_ctx(|ctx| {
            super::extrusion_cap_pcurve(
                ctx,
                &arc,
                reversed,
                start,
                end,
                &"wrapped arc cap",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect("service allocation")
        .expect("quarter-turn cap geometry");
        // One quarter turn: the curve midpoint is the angular midpoint.
        for (parameter, expected) in [
            (0.0, start),
            (0.5, [FRAC_1_SQRT_2, FRAC_1_SQRT_2]),
            (1.0, end),
        ] {
            let point = cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &pcurve,
                parameter,
            )
            .expect("finite pcurve point");
            assert!(
                (point.u - expected[0]).abs() < EPS_QUARTER_CIRCLE_SEAM
                    && (point.v - expected[1]).abs() < EPS_QUARTER_CIRCLE_SEAM,
                "reversed {reversed} parameter {parameter}: {point:?}"
            );
        }
    }
}

#[test]
fn overflowing_circular_pcurve_records_refusal_text() {
    const REASON: &str = "control_points contains a non-finite point";

    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    let absent = crate::decode::with_test_decode_ctx(|ctx| {
        super::circular_pcurve(
            ctx,
            [f64::MAX, f64::MAX],
            f64::MAX,
            0.0,
            std::f64::consts::TAU,
            &"extrusion feature 11 cap",
            &mut refusal,
        )
    })
    .expect("service refusal text is admitted");
    assert!(absent.is_none());
    assert_eq!(
        refusal.take_records(),
        [format!(
            "creo circular pcurve record for extrusion feature 11 cap: {REASON}"
        )]
    );
}

#[test]
fn two_refused_circular_pcurves_state_two_records_each_naming_its_instance() {
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    let first = crate::decode::with_test_decode_ctx(|ctx| {
        super::circular_pcurve(
            ctx,
            [f64::MAX, f64::MAX],
            f64::MAX,
            0.0,
            std::f64::consts::TAU,
            &"extrusion feature 11 cap",
            &mut refusal,
        )
    })
    .expect("resource admission");
    let second = crate::decode::with_test_decode_ctx(|ctx| {
        super::circular_pcurve(
            ctx,
            [f64::MAX, f64::MAX],
            f64::MAX,
            0.0,
            std::f64::consts::TAU,
            &"extrusion feature 12 cap",
            &mut refusal,
        )
    })
    .expect("resource admission");
    assert!(first.is_none(), "the refused arc states no pcurve");
    assert!(second.is_none(), "the refused arc states no pcurve");
    let records = refusal.take_records();
    assert_eq!(records.len(), 2, "one record per refused arc: {records:?}");
    assert!(
        records[0].starts_with("creo circular pcurve record for extrusion feature 11 cap: "),
        "the record names the instance that stated the lanes: {}",
        records[0]
    );
    assert!(
        records[1].starts_with("creo circular pcurve record for extrusion feature 12 cap: "),
        "the record names the instance that stated the lanes: {}",
        records[1]
    );
}

#[test]
fn circular_pcurve_skips_unbounded_span_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    for (start, end) in [
        (0.0, 100_001.0 * std::f64::consts::FRAC_PI_2),
        (0.0, 1.0e20),
        (0.0, f64::INFINITY),
        (-f64::MAX, f64::MAX),
    ] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let pcurve = super::circular_pcurve(
            &ctx,
            [0.0, 0.0],
            1.0,
            start,
            end,
            &"oversized arc",
            &mut refusal,
        )
        .expect("the segment ceiling skips the arc before allocation");
        assert!(pcurve.is_none());
        assert!(ctx.resource_refusal().is_none());
        let records = refusal.take_records();
        assert_eq!(records.len(), 1);
        assert!(
            records[0].starts_with("creo circular pcurve record for oversized arc: ")
                && records[0].contains("100000 quarter-turn segments"),
            "{}",
            records[0]
        );
    }
}

#[test]
fn overflowing_profile_area_is_not_validated() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let edge = |start: [f64; 2], end: [f64; 2]| {
        super::ProfileEntity::new(
            &ctx,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .expect("finite sketch line"),
            false,
        )
        .expect("service profile resources")
        .expect("profile entity")
    };
    let origin = [0.0, 0.0];
    let right = [1.0e154, 0.0];
    let upper = [0.0, 1.0e154];
    let circuit = [edge(origin, right), edge(right, upper), edge(upper, origin)];
    let profile: Vec<_> = circuit.clone().into_iter().chain(circuit).collect();
    assert!(super::extrusion_profile_signed_area(&ctx, &profile)
        .expect("service area resources")
        .is_none());
    assert!(super::ValidatedProfile::new(&ctx, profile)
        .expect("service validation resources")
        .is_none());
}

#[test]
fn large_profile_circle_intersections_stay_finite() {
    let r = 1e200;
    let arc = ([0., 0.], r, 0., std::f64::consts::TAU);
    assert!(super::line_arc_intersect(
        [[-2. * r, 0.], [2. * r, 0.]],
        arc,
        1e-9,
        [None, None]
    ));
    assert!(super::arcs_intersect(
        arc,
        ([r, 0.], r, 0., std::f64::consts::TAU),
        1e-9,
        [None, None]
    ));
    assert!(!super::arcs_intersect(
        arc,
        ([3. * r, 0.], r, 0., std::f64::consts::TAU),
        1e-9,
        [None, None]
    ));
}

#[test]
fn small_segment_crossings_do_not_depend_on_cross_product_units() {
    const DISTANCE_TOLERANCE: f64 = 1e-9;
    for scale in [1e-5, 1.0, 1e200] {
        assert!(super::segments_intersect(
            [[-scale, 0.0], [scale, 0.0]],
            [[0.0, -scale], [0.0, scale]],
            DISTANCE_TOLERANCE,
            [None, None]
        ));
    }
    assert!(!super::segments_intersect(
        [[0.0, 0.0], [1e-5, 0.0]],
        [[0.0, 1e-5], [1e-5, 1e-5]],
        DISTANCE_TOLERANCE,
        [None, None]
    ));
}

#[test]
fn numerical_ranges_profile_arc_tolerance_is_a_length_at_both_ends() {
    const RADIUS: f64 = 1e-6;
    const TOLERANCE: f64 = 1e-9;
    for sweep in [-1.0_f64, 1.0] {
        for angle in [-0.0005 * sweep, 1.0005 * sweep] {
            let point = [RADIUS * angle.cos(), RADIUS * angle.sin()];
            assert!(super::point_on_profile_arc(
                point,
                ([0., 0.], RADIUS, 0., sweep),
                TOLERANCE
            ));
        }
        let angle = 1.01 * sweep;
        assert!(!super::point_on_profile_arc(
            [RADIUS * angle.cos(), RADIUS * angle.sin()],
            ([0., 0.], RADIUS, 0., sweep),
            TOLERANCE
        ));
    }
}

#[test]
fn audit_regression_line_arc_endpoint_tolerance_has_length_units() {
    let line = [[0., 0.], [1000., 0.]];
    let arc = |center| ([center, 0.], 0.1, 0., std::f64::consts::TAU);
    assert!(!super::line_arc_intersect(
        line,
        arc(1000.5),
        0.001,
        [None, None]
    ));
    assert!(super::line_arc_intersect(
        line,
        arc(1000.1005),
        0.001,
        [None, None]
    ));
    assert!(super::line_arc_intersect(
        line,
        arc(999.5),
        0.001,
        [None, None]
    ));
}

#[test]
fn resolved_profile_nurbs_copy_keeps_knots_and_poles() {
    for rational in [false, true] {
        let sketch_id = SketchId::mint("creo:model:sketch#74").expect("ID");
        let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#74:1").expect("ID");
        let curve = PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(0.0, 0.0),
            ],
            rational.then(|| vec![1.0, 2.0, 1.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .expect("curve");
        let mut ir = CadIr::empty();
        ir.model.sketches.push(sketch(&sketch_id, &entity_id));
        ir.model.sketch_entities.push(SketchEntity::new(
            entity_id,
            sketch_id.clone(),
            SketchGeometry::nurbs(curve.clone()),
        ));
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let profiles = crate::decode::with_test_decode_ctx(|ctx| {
            super::resolved_sketch_profiles(ctx, &ir, &carriers, &sketch_id, 1)
        })
        .expect("service")
        .expect("profile");
        assert!(
            matches!(profiles[0][0].geometry(), super::ProfileGeometry::Nurbs { curve: copied, .. } if copied == &curve)
        );
    }
}

#[test]
fn nurbs_profile_local_depth_ceiling_leaves_the_span_unsampled() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("curved fixture");
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut evaluator =
            cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(ctx, &curve).expect("evaluator");
        let mut points = Vec::new();
        let sampled = super::append_nurbs_profile_span(
            ctx,
            &mut evaluator,
            &curve,
            &super::NurbsProfileSpan {
                start: 0.0,
                end: 1.0,
                start_point: [0.0, 0.0],
                end_point: [1.0, 1.0],
                tolerance: f64::MIN_POSITIVE,
                depth: 24,
            },
            &mut points,
        )
        .expect("the local depth ceiling is not a decode failure");
        assert_eq!(sampled, None);
        assert!(points.is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn nurbs_profile_local_point_ceiling_leaves_the_span_unsampled() {
    let curve = linear_profile_curve();
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut evaluator =
            cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(ctx, &curve).expect("evaluator");
        let mut points = vec![[0.0, 0.0]; 262_145];
        let sampled = super::append_nurbs_profile_span(
            ctx,
            &mut evaluator,
            &curve,
            &super::NurbsProfileSpan {
                start: 0.0,
                end: 1.0,
                start_point: [0.0, 0.0],
                end_point: [1.0, 0.0],
                tolerance: 0.01,
                depth: 0,
            },
            &mut points,
        )
        .expect("the local point ceiling is not a decode failure");
        assert_eq!(sampled, None);
        assert_eq!(points.len(), 262_145);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn nurbs_profile_point_append_stops_before_growth_at_the_common_ceiling() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut points = vec![[0.0; 2]; super::MAX_NURBS_PROFILE_POINTS];
        let appended = super::append_nurbs_profile_point(&mut points, [1.0; 2]);
        assert_eq!(appended, None);
        assert!(ctx.resource_refusal().is_none());
        assert_eq!(points.len(), super::MAX_NURBS_PROFILE_POINTS);
        assert_eq!(points.last(), Some(&[0.0; 2]));
    });
}

mod work_admission;
