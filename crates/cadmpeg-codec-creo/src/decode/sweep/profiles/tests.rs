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
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("linear NURBS fixture");
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::nurbs_profile_polyline(ctx, &nurbs, 0.01))
            .expect("service profile resources"),
        Some(vec![[0.0, 0.0], [1.0, 0.0]])
    );
}

fn linear_profile_curve() -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
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
fn nurbs_profile_polyline_first_point_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error =
        linear_profile_polyline_with_policy(policy).expect_err("first point exceeds zero items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo NURBS profile polyline points")
    );
    assert_eq!(
        linear_profile_polyline_with_policy(DecodePolicy::service()).expect("service polyline"),
        Some(vec![[0.0, 0.0], [1.0, 0.0]])
    );
}

#[test]
fn nurbs_profile_polyline_next_point_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error =
        linear_profile_polyline_with_policy(policy).expect_err("second point exceeds one item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo NURBS profile polyline points")
    );
    assert_eq!(
        linear_profile_polyline_with_policy(DecodePolicy::service()).expect("service polyline"),
        Some(vec![[0.0, 0.0], [1.0, 0.0]])
    );
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

#[test]
fn nurbs_profile_polyline_work_refuses_before_span_evaluation() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error =
        linear_profile_polyline_with_policy(policy).expect_err("first span exceeds zero work");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::WorkUnits
            && refusal.operation == "creo NURBS profile sampling spans")
    );
    assert_eq!(
        linear_profile_polyline_with_policy(DecodePolicy::service()).expect("service polyline"),
        Some(vec![[0.0, 0.0], [1.0, 0.0]])
    );
}

fn profile_sketch_copy_with_limit(
    limit: u64,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    let geometry = super::ProfileGeometry::Nurbs {
        curve: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .expect("linear pcurve fixture"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    geometry.to_sketch(&ctx)
}

#[test]
fn profile_sketch_copy_knots_refuse_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let error = profile_sketch_copy_with_limit(0).expect_err("four knots exceed zero items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo profile sketch NURBS knots")
    );
    assert!(profile_sketch_copy_with_limit(6)
        .expect("service sized copy")
        .is_some());
}

#[test]
fn profile_sketch_copy_poles_refuse_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let error = profile_sketch_copy_with_limit(4).expect_err("two poles exceed four knot items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo profile sketch NURBS poles")
    );
    assert!(profile_sketch_copy_with_limit(6)
        .expect("service sized copy")
        .is_some());
}

fn ordered_circle_with_limit(
    limit: u64,
) -> Result<Option<Vec<super::ValidatedProfile>>, cadmpeg_core::CodecError> {
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(0.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(1.0).expect("positive radius"),
    })
    .expect("valid circle fixture");
    let entity =
        crate::decode::with_test_decode_ctx(|ctx| super::ProfileEntity::new(ctx, geometry, false))
            .expect("service entity resources")
            .expect("circle profile entity");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    super::ordered_extrusion_profiles(&ctx, vec![vec![entity]])
}

fn under_work_limit<T>(
    limit: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    run(&ctx)
}

fn profile_circle(center: [f64; 2], radius: f64, reversed: bool) -> super::ProfileEntity {
    super::ProfileEntity {
        geometry: super::ProfileGeometry::Circle {
            center: Point2::new(center[0], center[1]),
            radius: cadmpeg_ir::scalar::Length::new(radius).expect("positive radius"),
        },
        reversed,
        start: [center[0] + radius, center[1]],
        end: [center[0] + radius, center[1]],
    }
}

fn profile_line(start: [f64; 2], end: [f64; 2]) -> super::ProfileEntity {
    super::ProfileEntity {
        geometry: super::ProfileGeometry::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        },
        reversed: false,
        start,
        end,
    }
}

fn profile_nurbs_line() -> super::ProfileEntity {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("linear profile pcurve");
    super::ProfileEntity {
        geometry: super::ProfileGeometry::Nurbs { curve },
        reversed: false,
        start: [0.0, 0.0],
        end: [1.0, 0.0],
    }
}

fn assert_work(error: &cadmpeg_core::CodecError, operation: &str) {
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == operation)
    );
}

#[test]
fn profile_polyline_pairs_refuse_work_limit() {
    let error = under_work_limit(0, |ctx| {
        super::polylines_intersect(
            ctx,
            &[[0.0, 0.0], [1.0, 0.0]],
            &[[0.0, 1.0], [1.0, 1.0]],
            0.0,
        )
    })
    .expect_err("one segment pair exceeds zero work");
    assert_work(&error, "creo profile polyline intersection pairs");
}

#[test]
fn profile_segment_intersection_refuses_work_limit() {
    let first = profile_line([0.0, 0.0], [1.0, 0.0]);
    let second = profile_line([0.0, 1.0], [1.0, 1.0]);
    let error = under_work_limit(0, |ctx| {
        super::profile_segments_intersect(ctx, &first, &second, 0.0)
    })
    .expect_err("one geometry pair exceeds zero work");
    assert_work(&error, "creo profile segment intersection");
}

#[test]
fn profile_nurbs_arc_intersection_refuses_segment_work() {
    let first = profile_nurbs_line();
    let second = profile_circle([10.0, 10.0], 1.0, false);
    let error = under_work_limit(2, |ctx| {
        super::profile_segments_intersect(ctx, &first, &second, 0.01)
    })
    .expect_err("NURBS arc segment exceeds prior pair and sample work");
    assert_work(&error, "creo profile NURBS arc intersection segments");
}

#[test]
fn profile_nurbs_winding_refuses_segment_work() {
    let profile = vec![profile_nurbs_line()];
    let error = under_work_limit(1, |ctx| {
        super::profile_strictly_contains(ctx, &profile, [0.5, 0.5])
    })
    .expect_err("NURBS winding segment exceeds prior sample work");
    assert_work(&error, "creo profile NURBS winding segments");
}

#[test]
fn profile_line_winding_refuses_work_limit() {
    let profile = vec![profile_line([0.0, 0.0], [1.0, 0.0])];
    let error = under_work_limit(0, |ctx| {
        super::profile_strictly_contains(ctx, &profile, [0.5, 0.5])
    })
    .expect_err("one line exceeds zero work");
    assert_work(&error, "creo profile line winding segments");
}

#[test]
fn profile_arc_winding_refuses_work_limit() {
    let profile = vec![profile_circle([0.0, 0.0], 1.0, false)];
    let error = under_work_limit(3, |ctx| {
        super::profile_strictly_contains(ctx, &profile, [0.0, 0.0])
    })
    .expect_err("four arc pieces exceed three work units");
    assert_work(&error, "creo profile arc winding pieces");
}

#[test]
fn profile_self_pairs_refuse_work_limit() {
    let profile = vec![
        profile_line([0.0, 0.0], [1.0, 0.0]),
        profile_line([1.0, 0.0], [1.0, 1.0]),
        profile_line([1.0, 1.0], [0.0, 1.0]),
        profile_line([0.0, 1.0], [0.0, 0.0]),
    ];
    let error = under_work_limit(0, |ctx| {
        super::ordered_extrusion_profiles(ctx, vec![profile])
    })
    .expect_err("one nonadjacent pair exceeds zero work");
    assert_work(&error, "creo profile self intersection pairs");
}

#[test]
fn profile_cross_pairs_refuse_work_limit() {
    let profiles = vec![
        vec![profile_circle([0.0, 0.0], 5.0, false)],
        vec![profile_circle([2.0, 0.0], 1.0, true)],
    ];
    let error = under_work_limit(0, |ctx| super::ordered_extrusion_profiles(ctx, profiles))
        .expect_err("one cross-profile pair exceeds zero work");
    assert_work(&error, "creo profile cross intersection pairs");
}

#[test]
fn outer_profile_containment_pairs_refuse_work_limit() {
    let profiles = vec![
        vec![profile_circle([0.0, 0.0], 5.0, false)],
        vec![profile_circle([2.0, 0.0], 1.0, true)],
    ];
    let error = under_work_limit(2, |ctx| super::ordered_extrusion_profiles(ctx, profiles))
        .expect_err("containment pair exceeds prior cross-pair work");
    assert_work(&error, "creo outer profile containment pairs");
}

#[test]
fn hole_profile_containment_pairs_refuse_work_limit() {
    let profiles = || {
        vec![
            vec![profile_circle([0.0, 0.0], 5.0, false)],
            vec![profile_circle([-2.0, 0.0], 1.0, true)],
            vec![profile_circle([2.0, 0.0], 1.0, true)],
        ]
    };
    let error = under_work_limit(26, |ctx| super::ordered_extrusion_profiles(ctx, profiles()))
        .expect_err("hole containment pair exceeds 26 prior work units");
    assert_work(&error, "creo hole profile containment pairs");
    assert!(
        under_work_limit(100, |ctx| super::ordered_extrusion_profiles(
            ctx,
            profiles()
        ))
        .expect("service work budget")
        .is_some()
    );
}

#[test]
fn outer_extrusion_candidate_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let error = ordered_circle_with_limit(0).expect_err("one outer candidate exceeds zero items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo outer extrusion profile candidates")
    );
    assert!(ordered_circle_with_limit(2)
        .expect("service ordering")
        .is_some());
}

#[test]
fn validated_extrusion_profile_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let error =
        ordered_circle_with_limit(1).expect_err("validation exceeds the outer candidate item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo validated extrusion profiles")
    );
    assert!(ordered_circle_with_limit(2)
        .expect("service ordering")
        .is_some());
}

#[test]
fn connected_profile_vertices_refuse_each_collection_boundary() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let sketch_id = SketchId::mint("creo:model:sketch#71").expect("identity grammar");
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#71:1").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model
        .sketch_entities
        .push(line_entity(&entity_id, &sketch_id, [1.0, 0.0]));
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let arena = DecodeArena::new();
    for (limit, operation) in [
        (0, "creo connected profile uses"),
        (1, "creo connected profile vertices"),
        (2, "creo connected profile vertices"),
        (3, "creo connected profile rows"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::connected_sketch_profile_vertices(&ctx, &ir, &carriers, &sketch_id)
            .map(std::iter::Iterator::collect::<Vec<_>>)
            .expect_err("collection limit refuses profile vertices");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }
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
fn resolved_nurbs_profile_refuses_source_geometry_copy_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let sketch_id = SketchId::mint("creo:model:sketch#74").expect("identity grammar");
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#74:1").expect("identity grammar");
    let nurbs = PcurveNurbs::from_lanes(
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
    .expect("closed source NURBS");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id,
        sketch_id.clone(),
        SketchGeometry::nurbs(nurbs),
    ));
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 8;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = super::resolved_sketch_profiles(&ctx, &ir, &carriers, &sketch_id, 1)
        .expect_err("nine knot and pole values exceed the limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo resolved profile NURBS copy"),
        "{error:?}"
    );
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
fn resolved_profile_refuses_entity_and_row_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
    let arena = DecodeArena::new();
    for (limit, operation) in [
        (0, "creo resolved profile entities"),
        (1, "creo resolved profile rows"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::resolved_sketch_profiles(&ctx, &ir, &carriers, &sketch_id, 1)
            .expect_err("profile collection exceeds its limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }
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
            1,
            vec![-f64::MAX, -f64::MAX, f64::MAX, f64::MAX],
            vec![Point2::new(1.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
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
fn circular_pcurve_refuses_each_counted_lane_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    for (limit, operation) in [
        (2, "creo circular pcurve controls"),
        (5, "creo circular pcurve weights"),
        (11, "creo circular pcurve knots"),
        (12, "creo circular pcurve weighted poles"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::circular_pcurve(
            &ctx,
            [0.0, 0.0],
            1.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            &"quarter circle",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect_err("counted circular lane exceeds its limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }
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
        cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("start"),
        Point2::new(1.0, 0.0)
    );
    let end = cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("end");
    assert!(end.u.abs() < EPS_QUARTER_CIRCLE_SEAM);
    assert!((end.v - 1.0).abs() < EPS_QUARTER_CIRCLE_SEAM);
}

#[test]
fn overflowing_circular_pcurve_refuses_before_error_text_copy() {
    const REASON: &str = "control_points contains a non-finite point";

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut policy = DecodePolicy::service();

    policy.limits.max_retained_bytes = u64::try_from(REASON.len()).expect("reason length") - 1;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    let error = super::circular_pcurve(
        &ctx,
        [f64::MAX, f64::MAX],
        f64::MAX,
        0.0,
        std::f64::consts::TAU,
        &"extrusion feature 11 cap",
        &mut refusal,
    )
    .expect_err("need minus one refuses before error text");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("retained refusal expected");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "creo circular pcurve refusal text");
    assert_eq!(limit.used, 0);
    assert_eq!(limit.limit + 1, limit.additional);
    assert!(refusal.take_records().is_empty());
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
fn circular_pcurve_refuses_unbounded_span_before_allocation() {
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::circular_pcurve(
            ctx,
            [0.0, 0.0],
            1.0,
            0.0,
            1.0e20,
            &"oversized arc",
            &mut refusal,
        ))
        .expect("resource admission")
        .is_none()
    );
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
        1e-9
    ));
    assert!(super::arcs_intersect(
        arc,
        ([r, 0.], r, 0., std::f64::consts::TAU),
        1e-9
    ));
    assert!(!super::arcs_intersect(
        arc,
        ([3. * r, 0.], r, 0., std::f64::consts::TAU),
        1e-9
    ));
}

#[test]
fn small_segment_crossings_do_not_depend_on_cross_product_units() {
    const DISTANCE_TOLERANCE: f64 = 1e-9;
    for scale in [1e-5, 1.0, 1e200] {
        assert!(super::segments_intersect(
            [[-scale, 0.0], [scale, 0.0]],
            [[0.0, -scale], [0.0, scale]],
            DISTANCE_TOLERANCE
        ));
    }
    assert!(!super::segments_intersect(
        [[0.0, 0.0], [1e-5, 0.0]],
        [[0.0, 1e-5], [1e-5, 1e-5]],
        DISTANCE_TOLERANCE
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
    assert!(!super::line_arc_intersect(line, arc(1000.5), 0.001));
    assert!(super::line_arc_intersect(line, arc(1000.1005), 0.001));
    assert!(super::line_arc_intersect(line, arc(999.5), 0.001));
}

#[test]
fn resolved_profile_nurbs_copy_refuses_knots_and_poles_separately() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    for rational in [false, true] {
        let sketch_id = SketchId::mint("creo:model:sketch#74").expect("ID");
        let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#74:1").expect("ID");
        let curve = PcurveNurbs::from_lanes(
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
        .expect("curve");
        let mut ir = CadIr::empty();
        ir.model.sketches.push(sketch(&sketch_id, &entity_id));
        ir.model.sketch_entities.push(SketchEntity::new(
            entity_id,
            sketch_id.clone(),
            SketchGeometry::nurbs(curve.clone()),
        ));
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        for cap in [5, 8] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            assert!(
                matches!(super::resolved_sketch_profiles(&ctx, &ir, &carriers, &sketch_id, 1),
                Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "creo resolved profile NURBS copy")
            );
        }
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
