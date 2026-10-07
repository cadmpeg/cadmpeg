// SPDX-License-Identifier: Apache-2.0
use super::bilinear_surface;
use crate::eval::nurbs_surface_parameter_segment_chord_bound;
use crate::eval::rational_patch_parameter_segment;
use crate::eval::RationalBezierSurfacePatch;
use crate::math::{Point2, Point3};
use crate::topology::IncreasingParameterInterval;
use crate::units::FinitePoint2;

#[test]
fn rational_patch_rejects_out_of_domain_endpoint_instead_of_moving_it() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let patch = RationalBezierSurfacePatch {
        u_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        v_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        u_degree: 1,
        v_degree: 1,
        controls: [[0.0, 0.0, 0.0, 1.0]; 4].to_vec(),
        _scratch: ctx.reserve_scoped_limit(0, "test patch controls").unwrap(),
    };
    let start = FinitePoint2::new(Point2::new(-0.5, 0.0)).unwrap();
    let end = FinitePoint2::new(Point2::new(0.5, 1.0)).unwrap();
    assert!(rational_patch_parameter_segment(
        &cadmpeg_test_support::service_decode_context(),
        &patch,
        start,
        end
    )
    .expect("resource allocation did not fail")
    .is_none());
}

#[test]
fn nurbs_surface_parameter_segment_bound_contains_curved_diagonal() {
    let mut surface = bilinear_surface();
    surface
        .try_map_control_points(
            |index, point| {
                let mut mapped = point.get();
                if index == 3 {
                    mapped.z = 1.0;
                }
                crate::features::FinitePoint3::new(mapped).ok_or(())
            },
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("pole edit admission")
        .unwrap();
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)];
    let bound = nurbs_surface_parameter_segment_chord_bound(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        parameters,
        chord,
    )
    .expect("resource allocation did not fail")
    .expect("rational Bézier residual bound");

    assert!(bound >= 1.0 / 3.0);
    assert!(bound < 1.0 / 3.0 + 1.0e-12);
    let reverse_bound = nurbs_surface_parameter_segment_chord_bound(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        [parameters[1], parameters[0]],
        [chord[1], chord[0]],
    )
    .expect("resource allocation did not fail")
    .expect("reversed rational Bézier residual bound");
    assert!((reverse_bound - bound).abs() < 1.0e-12);
    for index in 0..=100 {
        let parameter = f64::from(index) / 100.0;
        let point = crate::eval::decode::nurbs_surface_point(
            crate::eval::admission::EvaluationAdmission::Standard,
            &surface,
            parameter,
            parameter,
        )
        .expect("surface point");
        let target = Point3::new(parameter, parameter, parameter);
        let distance = (point.x - target.x)
            .hypot(point.y - target.y)
            .hypot(point.z - target.z);
        assert!(distance <= bound);
    }
}

#[test]
fn surface_segment_bound_preserves_session_and_local_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)];
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = nurbs_surface_parameter_segment_chord_bound(&ctx, &surface, parameters, chord)
            .unwrap_err();
        assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
        assert_eq!(
            ctx.finish_session().unwrap_err().to_string(),
            error.to_string()
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        nurbs_surface_parameter_segment_chord_bound(&ctx, &surface, parameters, chord)
            .unwrap()
            .is_some()
    );
    ctx.finish_session().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let budget = ctx.work_budget(0);
    let error = crate::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
        &ctx, &surface, parameters, chord, &budget,
    )
    .unwrap_err();
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.operation == "IR surface segment work")
    );
    assert_eq!(
        ctx.finish_session().unwrap_err().to_string(),
        error.to_string()
    );
}

#[test]
fn surface_patch_extraction_preserves_refusals_and_scoped_output_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!("tested extraction dimensions"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let budget = ctx.work_budget(1_000_000);
        let limit = crate::eval::rational_surface_patches_with_budget(&ctx, &surface, &budget)
            .expect_err("extraction admission");
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 65536;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let budget = ctx.work_budget(1_000_000);
    let output = crate::eval::rational_surface_patches_with_budget(&ctx, &surface, &budget)
        .expect("extraction")
        .expect("active patch");
    assert_eq!(output.rows.len(), 1);
    assert_eq!(output.rows[0].u_domain.endpoints(), [0.0, 1.0]);
    assert_eq!(output.rows[0].v_domain.endpoints(), [0.0, 1.0]);
    assert_eq!(
        output.rows[0].controls,
        [
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        ]
    );
    let bytes = output.rows.capacity() * std::mem::size_of::<RationalBezierSurfacePatch>()
        + output
            .rows
            .iter()
            .map(|patch| patch.controls.capacity() * std::mem::size_of::<[f64; 4]>())
            .sum::<usize>();
    let bytes = u64::try_from(bytes).expect("test allocation bytes fit");
    let spare = ctx
        .reserve_scoped_limit(65536 - bytes, "test surface extraction scratch released")
        .expect("only output headers and controls remain");
    drop(spare);
    drop(output);
    let reuse = ctx
        .reserve_scoped_limit(65536, "test surface patches released")
        .expect("all bytes reusable");
    drop(reuse);
    ctx.finish_session()
        .expect("no retained extraction storage");
    let ctx = cadmpeg_test_support::service_decode_context();
    let budget = cadmpeg_core::decode::WorkBudget::new(0);
    assert!(
        crate::eval::rational_surface_patches_with_budget(&ctx, &surface, &budget)
            .expect("independent local exhaustion")
            .is_none()
    );
    ctx.finish_session()
        .expect("local exhaustion is not a session refusal");
}

#[test]
fn surface_patch_split_preserves_refusals_and_both_tensor_directions() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let source_budget = source_ctx.work_budget(1_000_000);
    let source =
        crate::eval::rational_surface_patches_with_budget(&source_ctx, &surface, &source_budget)
            .expect("fixture extraction")
            .expect("active patch");
    for split_u in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!("tested split dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let budget = ctx.work_budget(1_000_000);
            let limit =
                crate::eval::split_rational_surface_patch(&ctx, &source.rows[0], split_u, &budget)
                    .expect_err("split admission");
            assert_eq!(limit.dimension, dimension);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 8192;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let budget = ctx.work_budget(1_000_000);
        let children =
            crate::eval::split_rational_surface_patch(&ctx, &source.rows[0], split_u, &budget)
                .expect("split")
                .expect("divisible patch");
        for (side, child) in children.iter().enumerate() {
            let range = if side == 0 { [0.0, 0.5] } else { [0.5, 1.0] };
            assert_eq!(
                child.u_domain.endpoints(),
                if split_u { range } else { [0.0, 1.0] }
            );
            assert_eq!(
                child.v_domain.endpoints(),
                if split_u { [0.0, 1.0] } else { range }
            );
            let u = child.u_domain.endpoints();
            let v = child.v_domain.endpoints();
            for (index, control) in child.controls.iter().enumerate() {
                assert_eq!(*control, [u[index / 2], v[index % 2], 0.0, 1.0]);
            }
        }
        let bytes = children
            .iter()
            .map(|patch| patch.controls.capacity() * std::mem::size_of::<[f64; 4]>())
            .sum::<usize>();
        let bytes = u64::try_from(bytes).expect("test bytes fit");
        let spare = ctx
            .reserve_scoped_limit(8192 - bytes, "test split surface scratch released")
            .expect("only child controls remain");
        drop(spare);
        drop(children);
        let reuse = ctx
            .reserve_scoped_limit(8192, "test split surface outputs released")
            .expect("all bytes reusable");
        drop(reuse);
        ctx.finish_session().expect("no retained split storage");
    }
}

#[test]
fn surface_distance_bounds_preserve_each_control_scan_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let source_budget = source_ctx.work_budget(1_000_000);
    let source =
        crate::eval::rational_surface_patches_with_budget(&source_ctx, &surface, &source_budget)
            .expect("fixture extraction")
            .expect("active patch");
    // One patch visit and four control visits.
    for cap in 0..5 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let budget = ctx.work_budget(1_000_000);
        let limit =
            crate::eval::rational_patch_distance_bounds_with_budget(&ctx, &source.rows[0], &budget)
                .expect_err("each visit needs work");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 5;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let budget = ctx.work_budget(1_000_000);
    let (lower, diameter) =
        crate::eval::rational_patch_distance_bounds_with_budget(&ctx, &source.rows[0], &budget)
            .expect("five visits")
            .expect("finite hull");
    assert_eq!(lower, 0.0);
    assert!((diameter - 2.0_f64.sqrt()).abs() <= 4.0 * f64::EPSILON);
    ctx.finish_session()
        .expect("no scratch allocation for bounds");
}

#[test]
fn surface_start_search_keeps_only_output_storage_after_both_contracts() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let surface = bilinear_surface();
    for tolerance in [None, Some(0.0)] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 8192;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let budget = ctx.work_budget(1_000_000);
        let point = Point3::new(0.3, 0.7, if tolerance.is_some() { 0.0 } else { 0.2 });
        let starts = crate::eval::complete_nurbs_surface_starts(
            &ctx, &surface, point, None, tolerance, &budget,
        )
        .expect("actual scoped workspace fits")
        .expect("surface starts");
        assert!(!starts.is_empty());
        for start in starts.iter() {
            assert!((start.u - 0.3).abs() <= 64.0 * f64::EPSILON);
            assert!((start.v - 0.7).abs() <= 64.0 * f64::EPSILON);
        }
        let bytes = u64::try_from(starts.capacity() * std::mem::size_of::<FinitePoint2>())
            .expect("test allocation bytes fit");
        let spare = ctx
            .reserve_scoped_limit(8192 - bytes, "test surface start workspace released")
            .expect("only output starts remain reserved");
        drop(spare);
        drop(starts);
        let reuse = ctx
            .reserve_scoped_limit(8192, "test surface starts released")
            .expect("all bytes reusable");
        drop(reuse);
        ctx.finish_session().expect("no retained search storage");
    }
}

#[test]
fn surface_distance_bounds_stop_at_the_first_invalid_control() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let patch = RationalBezierSurfacePatch {
        u_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        v_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        u_degree: 1,
        v_degree: 1,
        controls: vec![
            [0.0, 0.0, 0.0, -1.0],
            [1.0, 1.0, 1.0, 1.0],
            [2.0, 2.0, 2.0, 1.0],
            [3.0, 3.0, 3.0, 1.0],
        ],
        _scratch: ctx.reserve_scoped_limit(0, "test patch controls").unwrap(),
    };
    assert_eq!(
        super::super::rational_patch_distance_bounds_with_budget(
            &ctx, &patch, &WorkBudget::new(1),
        ).unwrap(),
        None,
    );
    drop(patch);
    ctx.finish_session().unwrap();
}

#[test]
fn surface_segment_validation_preserves_named_work_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let surface = bilinear_surface();
    for operation in [
        "IR surface u span count scan",
        "IR surface v span count scan",
        "IR rational surface diagonal finite scan",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = nurbs_surface_parameter_segment_chord_bound(
                    &ctx,
                    &surface,
                    [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
                    [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                );
                if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                    assert!(matches!(ctx.finish_session(),
                        Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == *limit));
                }
                result
            },
        );
    }
}

#[test]
fn surface_patch_grid_lookup_keeps_first_match_at_shared_boundaries() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let ctx = cadmpeg_test_support::service_decode_context();
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 2.0, 2.0], false);
    let points = (0..3)
        .map(|u| {
            (0..3)
                .map(|v| Point3::new(f64::from(u), f64::from(v), 0.0))
                .collect()
        })
        .collect();
    let surface = NurbsSurface::from_lanes(
        &ctx,
        axis(),
        axis(),
        NurbsSurfaceLanes::new(points, None),
        false,
    )
    .unwrap()
    .unwrap();
    let budget = ctx.work_budget(1_000_000);
    let patches = super::super::rational_surface_patches_with_budget(&ctx, &surface, &budget)
        .unwrap()
        .unwrap();
    assert_eq!(patches.rows.len(), 4);
    assert_eq!(patches.v_span_count, 2);
    for u in [-0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5] {
        for v in [-0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5] {
            let expected = patches.rows.iter().find(|patch| {
                patch.u_domain.lower() <= u
                    && u <= patch.u_domain.upper()
                    && patch.v_domain.lower() <= v
                    && v <= patch.v_domain.upper()
            });
            let actual = patches
                .patch_at_parameters(&ctx, FinitePoint2::new(Point2::new(u, v)).unwrap())
                .unwrap();
            assert_eq!(
                actual.map(std::ptr::from_ref),
                expected.map(std::ptr::from_ref)
            );
        }
    }
}
