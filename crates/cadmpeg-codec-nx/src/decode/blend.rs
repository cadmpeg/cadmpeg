// SPDX-License-Identifier: Apache-2.0
//! Blend-surface evaluation, spine inversion, and closest-pcurve search.

#[cfg(test)]
use super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK;
use super::geometry_work::{same_text, GeometryWorkBudget};
use super::offset::offset_surface_parameters_with_tolerance_with_index_and_budget;
use super::offset::{
    coarse_model_surface_parameters, parameter_derivative_step,
    refine_offset_surface_parameters_with_index_and_budget, surface_parameter_domain_with_index,
};
use super::support_uv::parameterization_equivalent_surfaces_with_index;
#[cfg(test)]
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::model_surface_partials_by_id;
use cadmpeg_ir::eval::model_surface_point_by_id;
use cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget;
use cadmpeg_ir::eval::pcurve_tangent;
use cadmpeg_ir::eval::EvaluationFailure;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::nurbs::bezier::{
    homogeneous_spans, positive_controls, HomogeneousBezierSpan,
};
use cadmpeg_ir::geometry::nurbs::scoped::ScopedRows;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, pcurve::PcurveGeometry, BlendCrossSection, BlendRadiusLaw,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::solve::least_squares_step;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::VecDeque;

const EPS_BLEND_EXACT_GEOMETRY: f64 = 1.0e-12;

const BLEND_SECTION_CANONICAL_DOMAIN: [f64; 2] = [0.0, 1.0];
// A source intersection chart can continue across either finite blend rail.
// Admit one additional section on each side, then require point reproduction
// before transferring the continuation back into the chart.
const BLEND_SECTION_SOURCE_CONTINUATION_DOMAIN: [f64; 2] = [-1.0, 2.0];
const BLEND_SECTION_BOUNDARY_EPSILON: f64 = EPS_BLEND_EXACT_GEOMETRY;
const BLEND_INVERSE_MIN_SWEEP: f64 = EPS_BLEND_EXACT_GEOMETRY;

#[derive(Clone, Copy)]
enum BlendSectionDomain {
    Canonical,
    SourceContinuation,
}

impl BlendSectionDomain {
    fn bounds(self) -> [f64; 2] {
        match self {
            Self::Canonical => BLEND_SECTION_CANONICAL_DOMAIN,
            Self::SourceContinuation => BLEND_SECTION_SOURCE_CONTINUATION_DOMAIN,
        }
    }

    fn contains(self, parameter: f64) -> bool {
        let [lower, upper] = self.bounds();
        (lower..=upper).contains(&parameter)
    }

    fn clamp_near_boundary(self, parameter: f64) -> Option<f64> {
        let [lower, upper] = self.bounds();
        (lower - BLEND_SECTION_BOUNDARY_EPSILON..=upper + BLEND_SECTION_BOUNDARY_EPSILON)
            .contains(&parameter)
            .then(|| parameter.clamp(lower, upper))
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn numerical_followup_pcurve_inverse_preserves_parameter_units_and_large_offsets() {
        use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
        for domain in [1.0, 1e9, 1e200] {
            let line = PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0., 0., domain, domain],
                    vec![Point2::new(0., 0.), Point2::new(1., 0.)],
                    None,
                    false,
                )
                .expect("fixture pcurve construction admission")
                .unwrap(),
            };
            let result = super::closest_pcurve_parameter_from_seed(
                &cadmpeg_test_support::service_decode_context(),
                &line,
                Point2::new(0.5, 0.),
                0.2 * domain,
            )
            .expect("evaluator allocation succeeds")
            .unwrap();
            assert!((result / domain - 0.5).abs() < 16. * f64::EPSILON);
            let coarse = super::closest_pcurve_parameter_from_coarse_grid(
                &cadmpeg_test_support::service_decode_context(),
                &line,
                Point2::new(0.5, 1e200),
            )
            .expect("evaluator allocation succeeds")
            .unwrap();
            assert!((coarse / domain - 0.5).abs() < 16. * f64::EPSILON);
        }
    }

    #[test]
    fn numerical_ranges_periodic_blend_parameter_avoids_difference_overflow() {
        assert_eq!(
            super::canonical_periodic_parameter([-1e308, 0.], true, 1e308),
            -1e308
        );
        assert_eq!(
            super::canonical_periodic_parameter([-1e308, 0.], false, 1e308),
            1e308
        );
    }

    #[test]
    fn periodic_blend_lifts_a_finite_parameter_across_a_wide_domain() {
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                super::lift_periodic_parameters(
                    ctx,
                    vec![-f64::MAX],
                    [-f64::MAX, f64::MAX],
                    true,
                    Some(f64::MAX),
                )
                .expect("lift is admitted"),
                vec![f64::MAX]
            );
            assert_eq!(
                super::lift_periodic_parameters(
                    ctx,
                    vec![-f64::MAX],
                    [-f64::MAX, f64::MAX],
                    true,
                    Some(0.0),
                )
                .expect("lift is admitted"),
                vec![-f64::MAX]
            );
        });
    }

    use super::{
        BlendContactSeedCache, BlendSectionDomain, BlendSurfaceFrameCache,
        BLEND_SECTION_BOUNDARY_EPSILON, MAX_BLEND_BOUNDARY_POINT_CACHE_ENTRIES,
        MAX_BLEND_CONTACT_SEEDS, MAX_BLEND_SURFACE_FRAME_CACHE_ENTRIES,
    };
    use crate::decode::geometry_work::GeometryWorkBudget;
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    #[test]
    fn blend_surface_frame_cache_evicts_old_entries_at_its_bound() {
        crate::test_support::with_decode_context(|geometry_ctx| {
            let mut cache = BlendSurfaceFrameCache::default();
            let frame = (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                1.0,
            );
            for index in 0..MAX_BLEND_SURFACE_FRAME_CACHE_ENTRIES {
                let surface = SurfaceId::mint(format!("test:model:entity#surface-{index}"))
                    .expect("identity grammar");
                cache
                    .remember(
                        &surface,
                        cadmpeg_core::convert::f64_from_index(index)
                            .expect("bounded cache fixture index is exact"),
                        false,
                        frame,
                        &GeometryWorkBudget::from_context(
                            geometry_ctx,
                            cadmpeg_core::decode::u64_from_index(100),
                        ),
                    )
                    .expect("cache allocation succeeds");
            }
            let first = SurfaceId::mint("test:model:entity#surface-0").expect("identity grammar");
            let get = |cache: &BlendSurfaceFrameCache, surface, parameter| {
                cache
                    .get(geometry_ctx, surface, parameter, false)
                    .expect("cache lookup fits the service profile")
            };
            assert_eq!(get(&cache, &first, 0.0), Some(frame));

            let newest =
                SurfaceId::mint("test:model:entity#surface-newest").expect("identity grammar");
            cache
                .remember(
                    &newest,
                    0.0,
                    false,
                    frame,
                    &GeometryWorkBudget::from_context(
                        geometry_ctx,
                        cadmpeg_core::decode::u64_from_index(100),
                    ),
                )
                .expect("cache allocation succeeds");
            assert!(get(&cache, &first, 0.0).is_none());
            assert_eq!(get(&cache, &newest, 0.0), Some(frame));
            assert!(get(&cache, &newest, -0.0).is_none());
        });
    }

    #[test]
    fn blend_boundary_point_cache_evicts_old_entries_at_its_bound() {
        crate::test_support::with_decode_context(|geometry_ctx| {
            let mut cache = BlendSurfaceFrameCache::default();
            let point = Point3::new(1.0, 2.0, 3.0);
            for index in 0..MAX_BLEND_BOUNDARY_POINT_CACHE_ENTRIES {
                let surface =
                    SurfaceId::mint(format!("test:model:entity#boundary-surface-{index}"))
                        .expect("identity grammar");
                cache
                    .remember_boundary_point(
                        &surface,
                        cadmpeg_core::convert::f64_from_index(index)
                            .expect("bounded cache fixture index is exact"),
                        index % 2,
                        point,
                        &GeometryWorkBudget::from_context(
                            geometry_ctx,
                            cadmpeg_core::decode::u64_from_index(100),
                        ),
                    )
                    .expect("cache allocation succeeds");
            }

            let first =
                SurfaceId::mint("test:model:entity#boundary-surface-0").expect("identity grammar");
            let get = |cache: &BlendSurfaceFrameCache, surface, parameter, boundary| {
                cache
                    .get_boundary_point(geometry_ctx, surface, parameter, boundary)
                    .expect("cache lookup fits the service profile")
            };
            assert_eq!(get(&cache, &first, 0.0, 0), Some(point));

            let newest = SurfaceId::mint("test:model:entity#boundary-surface-newest")
                .expect("identity grammar");
            cache
                .remember_boundary_point(
                    &newest,
                    0.0,
                    1,
                    point,
                    &GeometryWorkBudget::from_context(
                        geometry_ctx,
                        cadmpeg_core::decode::u64_from_index(100),
                    ),
                )
                .expect("cache allocation succeeds");
            assert!(get(&cache, &first, 0.0, 0).is_none());
            assert_eq!(get(&cache, &newest, 0.0, 1), Some(point));
            assert!(get(&cache, &newest, 0.0, 0).is_none());
            assert!(get(&cache, &newest, -0.0, 1).is_none());
        });
    }

    #[test]
    fn blend_frame_cache_refuses_retained_identity_at_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let budget = GeometryWorkBudget::from_context(ctx, 100);
                let surface =
                    SurfaceId::mint("test:model:entity#blend-frame").expect("identity grammar");
                let frame = (
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    1.0,
                );
                let limit = BlendSurfaceFrameCache::default()
                    .remember(&surface, 0.0, false, frame, &budget)
                    .expect_err("frame identity exceeds retained limit");
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "nx blend frame cache identity");
            },
        );
    }

    #[test]
    fn blend_frame_cache_refuses_entry_at_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let budget = GeometryWorkBudget::from_context(ctx, 100);
                let surface =
                    SurfaceId::mint("test:model:entity#blend-frame").expect("identity grammar");
                let frame = (
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    1.0,
                );
                let limit = BlendSurfaceFrameCache::default()
                    .remember(&surface, 0.0, false, frame, &budget)
                    .expect_err("frame entry exceeds collection limit");
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                assert_eq!(limit.operation, "nx blend frame cache entries");
            },
        );
    }

    #[test]
    fn blend_section_boundary_clamps_only_nearby_roundoff() {
        assert_eq!(
            BlendSectionDomain::Canonical
                .clamp_near_boundary(-0.5 * BLEND_SECTION_BOUNDARY_EPSILON),
            Some(0.0)
        );
        assert_eq!(
            BlendSectionDomain::Canonical
                .clamp_near_boundary(1.0 + 0.5 * BLEND_SECTION_BOUNDARY_EPSILON),
            Some(1.0)
        );
        assert_eq!(
            BlendSectionDomain::Canonical.clamp_near_boundary(0.25),
            Some(0.25)
        );
        assert_eq!(
            BlendSectionDomain::Canonical
                .clamp_near_boundary(-2.0 * BLEND_SECTION_BOUNDARY_EPSILON),
            None
        );
        assert_eq!(
            BlendSectionDomain::Canonical
                .clamp_near_boundary(1.0 + 2.0 * BLEND_SECTION_BOUNDARY_EPSILON),
            None
        );
    }

    #[test]
    fn blend_contact_seed_cache_is_bounded_and_uses_the_nearest_chart() {
        crate::test_support::with_decode_context(|geometry_ctx| {
            let support = SurfaceId::mint("test:model:entity#synthetic:seed-support")
                .expect("identity grammar");
            let spine =
                CurveId::mint("test:model:entity#synthetic:seed-spine").expect("identity grammar");
            let offset_surface = SurfaceId::mint("test:model:entity#synthetic:seed-offset")
                .expect("identity grammar");
            let mut cache = BlendContactSeedCache::default();
            for parameter in 0..(MAX_BLEND_CONTACT_SEEDS + 4) {
                let parameter = cadmpeg_core::convert::f64_from_index(parameter)
                    .expect("fixture integer is exactly representable");
                cache
                    .remember(
                        (&support, &spine, &offset_surface),
                        parameter,
                        Point2::new(parameter, -parameter),
                        &GeometryWorkBudget::from_context(
                            geometry_ctx,
                            cadmpeg_core::decode::u64_from_index(100),
                        ),
                    )
                    .expect("cache allocation succeeds");
            }

            assert_eq!(cache.entries.len(), MAX_BLEND_CONTACT_SEEDS);
            assert_eq!(
                cache
                    .seed_for(geometry_ctx, &support, &spine, 7.1, &offset_surface)
                    .expect("seed lookup fits the service profile"),
                Some(Point2::new(7.0, -7.0))
            );
        });
    }

    #[test]
    fn blend_contact_seed_cache_retains_no_identity_text() {
        let support =
            SurfaceId::mint("test:model:entity#synthetic:seed-support").expect("identity grammar");
        let spine =
            CurveId::mint("test:model:entity#synthetic:seed-spine").expect("identity grammar");
        let offset_surface =
            SurfaceId::mint("test:model:entity#synthetic:seed-offset").expect("identity grammar");
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| {
                let geometry_budget = GeometryWorkBudget::from_context(ctx, 100);
                let mut cache = BlendContactSeedCache::default();
                for parameter in 0..(MAX_BLEND_CONTACT_SEEDS + 4) {
                    let parameter = cadmpeg_core::convert::f64_from_index(parameter)
                        .expect("fixture integer is exactly representable");
                    cache
                        .remember(
                            (&support, &spine, &offset_surface),
                            parameter,
                            Point2::new(parameter, -parameter),
                            &geometry_budget,
                        )
                        .expect("a seed retains no storage");
                }
                assert_eq!(cache.entries.len(), MAX_BLEND_CONTACT_SEEDS);
            },
        );
    }

    #[test]
    fn de_casteljau_work_charges_every_combination_and_refuses_overflow() {
        use cadmpeg_core::decode::ResourceDimension;

        const OPERATION: &str = "test de Casteljau work";
        for (count, pairs) in [(2, 1), (4, 6), (5, 10)] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                OPERATION,
                |ctx| super::charge_de_casteljau_work(ctx, count, OPERATION).map_err(Into::into),
            );
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.additional == pairs
            ));
        }
        crate::test_support::with_decode_context(|ctx| {
            let limit = super::charge_de_casteljau_work(ctx, usize::MAX, OPERATION)
                .expect_err("work beyond usize is refused");
            assert_eq!(limit.dimension, ResourceDimension::Codec(OPERATION));
            assert_eq!(limit.operation, OPERATION);
        });
    }

    #[test]
    fn de_casteljau_empty_and_single_control_charge_no_work() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                for count in [0, 1] {
                    super::charge_de_casteljau_work(ctx, count, "test zero de Casteljau work")
                        .expect("no combinations need work");
                }
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }

    #[test]
    fn de_casteljau_overflow_refuses_even_an_unlimited_work_allowance() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = u64::MAX,
            |ctx| {
                let limit = super::charge_de_casteljau_work(ctx, usize::MAX, "test pair overflow")
                    .expect_err("an overflowing bound cannot be admitted");
                assert_eq!(ctx.resource_refusal(), Some(limit));
            },
        );
    }

    #[test]
    fn polynomial_root_sorts_refuse_work_before_returning_roots() {
        use cadmpeg_core::decode::ResourceDimension;

        for (dimension, operation) in [
            (ResourceDimension::WorkUnits, "nx polynomial critical roots sort"),
            (ResourceDimension::WorkUnits, "nx polynomial roots sort"),
        ] {
            crate::test_support::resource_refusal_at(
                &[],
                dimension,
                operation,
                |ctx| super::real_polynomial_roots(ctx, &[-1.0, 3.5, -3.0, -0.5, 1.0]).map(|_| ()),
            );
        }
    }

    #[test]
    fn polynomial_root_sorts_need_no_scratch_for_a_quartic() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_materialized_bytes = 0,
            |ctx| {
                let roots = super::real_polynomial_roots(ctx, &[-1.0, 3.5, -3.0, -0.5, 1.0])
                    .expect("fixed-degree sorts need no scratch")
                    .expect("finite quartic roots");
                assert_eq!(roots.len(), 3);
            },
        );
    }

    #[test]
    fn numerical_seventh_common_weights_preserve_closest_point() {
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        crate::test_support::with_decode_context(|geometry_ctx| {
            for weight in [1.0e-200, 1.0, 1.0e200] {
                let controls = [
                    [-0.25 * weight, 0.0, 0.0, weight],
                    [0.75 * weight, 0.0, 0.0, weight],
                ];
                let distance = super::homogeneous_residual_distance(
                    &controls,
                    0.0,
                    [0.0, 1.0],
                    &GeometryWorkBudget::from_context(
                        geometry_ctx,
                        cadmpeg_core::decode::u64_from_index(100),
                    ),
                )
                .expect("test solver allocation succeeds");
                assert!((distance - 0.25).abs() <= 4.0 * f64::EPSILON);
                let curve = NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    Some(vec![weight; 2]),
                    false,
                )
                .expect("fixture constructor admission")
                .unwrap();
                let parameter = super::closest_nurbs_curve_parameter_with_budget(
                    &curve,
                    Point3::new(0.25, 0.0, 0.0),
                    None,
                    &super::GeometryWorkBudget::from_context(
                        geometry_ctx,
                        cadmpeg_core::decode::u64_from_index(super::MAX_ADAPTIVE_GEOMETRY_WORK),
                    ),
                )
                .expect("evaluator allocation succeeds")
                .unwrap();
                assert!((parameter - 0.25).abs() <= 128.0 * f64::EPSILON);
            }
        });
    }

    #[test]
    fn nurbs_closest_parameter_refuses_weight_scratch_at_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            Some(vec![1.0, 1.0]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid rational curve");

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 1;
            },
            |ctx| {
                let budget = super::GeometryWorkBudget::from_context(ctx, 8_000_000);
                let result = super::closest_nurbs_curve_parameter_with_budget(
                    &curve,
                    Point3::new(0.25, 0.0, 0.0),
                    None,
                    &budget,
                );
                let limit = result.expect_err("two weights exceed one collection item");
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                assert_eq!(limit.operation, "nx spine NURBS weights");
            },
        );
    }

    #[test]
    fn nurbs_closest_parameter_refuses_residual_scratch_at_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid polynomial curve");

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 1;
            },
            |ctx| {
                let budget = super::GeometryWorkBudget::from_context(ctx, 8_000_000);
                let result = super::closest_nurbs_curve_parameter_with_budget(
                    &curve,
                    Point3::new(0.25, 0.0, 0.0),
                    None,
                    &budget,
                );
                let limit = result.expect_err("two residuals exceed one collection item");
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                assert_eq!(limit.operation, "nx spine NURBS residuals");
            },
        );
    }

    #[test]
    fn nurbs_closest_parameter_refuses_session_work_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid polynomial curve");

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
            },
            |ctx| {
                let budget = super::GeometryWorkBudget::from_context(ctx, 8_000_000);
                let result = super::closest_nurbs_curve_parameter_with_budget(
                    &curve,
                    Point3::new(0.25, 0.0, 0.0),
                    None,
                    &budget,
                );
                let limit = result.expect_err("closest-parameter search needs work");
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            },
        );
    }

    #[test]
    fn nurbs_closest_parameter_refuses_local_geometry_work_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid polynomial curve");

        crate::test_support::with_decode_context(|ctx| {
            let budget = super::GeometryWorkBudget::from_context(ctx, 0);
            let result = super::closest_nurbs_curve_parameter_with_budget(
                &curve,
                Point3::new(0.25, 0.0, 0.0),
                None,
                &budget,
            );
            let limit = result.expect_err("closest-parameter search exceeds zero local work");
            assert_eq!(
                limit.dimension,
                ResourceDimension::Codec("nx adaptive geometry work")
            );
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    #[test]
    fn bezier_root_interval_end_probe_refuses_session_work_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        // The interval probe is one unit of the adaptive geometry budget; the
        // control copy and scan before it charge the session directly.
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "work_budget",
            |ctx| {
                let geometry_budget = super::GeometryWorkBudget::from_context(ctx, 100);
                let span = super::ScalarBezierSpan {
                    domain: [0.0, 1.0],
                    controls: super::ScopedValues::copy_of(ctx, &[1.0, 1.0], "test controls")?,
                };
                super::scalar_bezier_roots_with_budget(span, &geometry_budget)
                    .map(|_| ())
                    .map_err(Into::into)
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "work_budget"
        ));
    }
}

pub(super) fn decoded_surface_point_inner_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    // A non-finite point is returned as the evaluation reached it; only an
    // evaluation with no value falls back to the blend construction.
    match cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
        .within_work_slice(geometry_budget, |admission| {
            model_surface_point_by_id(admission, index, surface, u, v)
        }) {
        Ok(point) => Ok(Some(point.get())),
        Err(EvaluationFailure::NonFinite(point)) => Ok(Some(point)),
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
        Err(EvaluationFailure::NoValue) => blend_surface_point_inner_with_index_and_budget(
            index,
            surface,
            u,
            v,
            depth + 1,
            geometry_budget,
        ),
    }
}

pub(super) fn decoded_surface_point_with_geometry_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    // A non-finite point is returned as the evaluation reached it; only an
    // evaluation with no value falls back to the next route.
    let evaluated =
        match cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
            .within_work_slice(geometry_budget, |admission| {
                cadmpeg_ir::eval::decode::surface_point(admission, geometry, u, v)
            }) {
            Err(EvaluationFailure::NoValue) => {
                cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
                    .within_work_slice(geometry_budget, |admission| {
                        model_surface_point_by_id(admission, index, surface, u, v)
                    })
            }
            direct => direct,
        };
    match evaluated {
        Ok(point) => Ok(Some(point.get())),
        Err(EvaluationFailure::NonFinite(point)) => Ok(Some(point)),
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
        Err(EvaluationFailure::NoValue) => blend_surface_point_inner_with_index_and_budget(
            index,
            surface,
            u,
            v,
            depth + 1,
            geometry_budget,
        ),
    }
}

#[cfg(test)]
pub(super) fn blend_surface_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    blend_surface_parameters_inner(
        &index,
        surface,
        &BlendSurfaceFit {
            point,
            seed,
            fit_tolerance: None,
            grid: BlendParameterGrid::Build,
            section_domain: BlendSectionDomain::Canonical,
            depth: 0,
        },
        &geometry_budget,
    )
}

#[cfg(test)]
pub(super) fn blend_surface_parameters_for_fit(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    blend_surface_parameters_for_fit_with_grid(
        ctx,
        &index,
        surface,
        point,
        seed,
        fit_tolerance,
        BlendParameterGrid::Build,
    )
}

#[derive(Clone, Copy)]
pub(super) enum BlendParameterGrid<'a> {
    #[cfg(test)]
    Build,
    Disabled,
    Provided(&'a BlendSurfaceGrid),
}

/// Spine parameters a blend parameter grid samples.
const BLEND_GRID_SPINE_SAMPLES: usize = 9;

/// Section parameters a blend parameter grid samples at each spine parameter.
const BLEND_GRID_SECTION_SAMPLES: usize = 5;

/// The most samples a blend parameter grid holds.
const BLEND_GRID_SAMPLES: usize = BLEND_GRID_SPINE_SAMPLES * BLEND_GRID_SECTION_SAMPLES;

/// A fixed lattice of blend parameters and their surface points that seeds a
/// blend parameter inversion. Samples whose point does not evaluate are left
/// out, so the grid holds a prefix of its fixed slots.
#[derive(Clone, Copy)]
pub(super) struct BlendSurfaceGrid {
    samples: [(Point2, Point3); BLEND_GRID_SAMPLES],
    len: usize,
}

impl BlendSurfaceGrid {
    /// The parameters of the sample nearest `point`.
    fn closest_parameters(&self, point: Point3) -> Option<Point2> {
        self.samples
            .iter()
            .take(self.len)
            .min_by(|(_, first), (_, second)| {
                Point3::distance(*first, point).total_cmp(&Point3::distance(*second, point))
            })
            .map(|(parameters, _)| *parameters)
    }

    #[cfg(test)]
    pub(super) fn samples(&self) -> &[(Point2, Point3)] {
        &self.samples[..self.len]
    }
}

/// Blend parameter grids by blend surface identity. A grid is built on its
/// surface's first request, and a surface whose grid does not build keeps that
/// answer. The table is held under one scoped reservation until the cache is
/// dropped.
pub(super) struct BlendParameterGridCache<'k, 'ctx> {
    grids: std::collections::BTreeMap<&'k str, Option<BlendSurfaceGrid>>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'k, 'ctx> BlendParameterGridCache<'k, 'ctx> {
    pub(super) fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            grids: std::collections::BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "nx blend parameter grid cache")?,
        })
    }

    /// The grid of `surface`, built on the surface's first request.
    pub(super) fn grid(
        &mut self,
        index: &cadmpeg_ir::index::ModelIndex<'_>,
        surface: &'k SurfaceId,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<BlendParameterGrid<'_>, cadmpeg_core::CodecError> {
        let ctx = geometry_budget.charges;
        if !ctx.contains_key_btree_map(
            &self.grids,
            surface.as_str(),
            "nx blend parameter grid lookup",
        )? {
            let grid = blend_surface_parameter_grid_with_index_and_budget(
                index,
                surface,
                0,
                geometry_budget,
            )?;
            let grids = &mut self.grids;
            self.storage.with_storage(|| {
                ctx.insert_btree_map(
                    grids,
                    surface.as_str(),
                    grid,
                    "nx blend parameter grid cache",
                )
            })?;
        }
        Ok(ctx
            .get_btree_map(
                &self.grids,
                surface.as_str(),
                "nx blend parameter grid lookup",
            )?
            .and_then(Option::as_ref)
            .map_or(BlendParameterGrid::Disabled, BlendParameterGrid::Provided))
    }
}

#[cfg(test)]
fn blend_surface_parameters_for_fit_with_grid(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    blend_surface_parameters_for_fit_with_grid_and_budget(
        index,
        surface,
        point,
        seed,
        fit_tolerance,
        grid,
        &geometry_budget,
    )
}

pub(super) fn blend_surface_parameters_for_fit_with_grid_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    blend_surface_parameters_for_fit_with_section_domain_and_budget(
        index,
        surface,
        &BlendSectionFit {
            point,
            seed,
            fit_tolerance,
            grid,
            section_domain: BlendSectionDomain::Canonical,
        },
        geometry_budget,
    )
}

pub(super) fn blend_surface_parameters_for_fit_with_source_continuation_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    blend_surface_parameters_for_fit_with_section_domain_and_budget(
        index,
        surface,
        &BlendSectionFit {
            point,
            seed,
            fit_tolerance,
            grid,
            section_domain: BlendSectionDomain::SourceContinuation,
        },
        geometry_budget,
    )
}

// The explicit search inputs keep canonical and source-continuation policies
// visible at every recursive boundary instead of hiding them in mutable state.

struct BlendSectionFit<'inputs> {
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid<'inputs>,
    section_domain: BlendSectionDomain,
}

fn blend_surface_parameters_for_fit_with_section_domain_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    inputs: &BlendSectionFit<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let &BlendSectionFit {
        point,
        seed,
        fit_tolerance,
        grid,
        section_domain,
    } = inputs;

    blend_surface_parameters_inner(
        index,
        surface,
        &BlendSurfaceFit {
            point,
            seed,
            fit_tolerance: Some(fit_tolerance),
            grid,
            section_domain,
            depth: 0,
        },
        geometry_budget,
    )
}

// The search state is explicit so every recursive evaluation shares the
// caller's model-wide work slice.

struct BlendSurfaceFit<'inputs> {
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
    grid: BlendParameterGrid<'inputs>,
    section_domain: BlendSectionDomain,
    depth: usize,
}

fn blend_surface_parameters_inner(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    blend_surface_fit: &BlendSurfaceFit<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let &BlendSurfaceFit {
        point,
        seed,
        fit_tolerance,
        grid,
        section_domain,
        depth,
    } = blend_surface_fit;

    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition { spine, .. }) =
        blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    if let (Some(seed), Some(fit_tolerance)) = (seed, fit_tolerance) {
        let seed_u_is_valid = index
            .curves(spine.as_str(), geometry_budget.charges)?
            .and_then(|curve| match curve.geometry.solved() {
                Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    let degree = usize::try_from(nurbs.degree()).ok()?;
                    let count = nurbs.pole_rows().count();
                    let lower = *nurbs.knots().get(degree)?;
                    let upper = *nurbs.knots().get(count)?;
                    Some(
                        lower.is_finite()
                            && upper.is_finite()
                            && lower < upper
                            && seed.u >= lower
                            && seed.u <= upper,
                    )
                }
                _ => Some(seed.u.is_finite()),
            })
            .unwrap_or(false);
        if seed_u_is_valid
            && section_domain.contains(seed.v)
            && blend_surface_point_inner_with_index_and_budget(
                index,
                surface,
                seed.u,
                seed.v,
                depth + 1,
                geometry_budget,
            )?
            .is_some_and(|candidate| Point3::distance(candidate, point) <= fit_tolerance)
        {
            return Ok(Some(seed));
        }
        if let Some(parameters) = refine_blend_surface_parameters_with_section_domain_and_budget(
            index,
            surface,
            point,
            seed,
            depth + 1,
            section_domain,
            geometry_budget,
        )? {
            if section_domain.contains(parameters.v)
                && blend_surface_point_inner_with_index_and_budget(
                    index,
                    surface,
                    parameters.u,
                    parameters.v,
                    depth + 1,
                    geometry_budget,
                )?
                .is_some_and(|candidate| Point3::distance(candidate, point) <= fit_tolerance)
            {
                return Ok(Some(parameters));
            }
        }
    }
    let angular_spine = closest_spine_parameter_with_index_and_budget(
        index,
        spine,
        point,
        seed.map(|seed| seed.u),
        geometry_budget,
    )?;
    let angular = if let Some(u) = angular_spine {
        if let Some((center, tangent, first, second, _)) =
            blend_surface_frame_with_index_and_budget(
                index,
                surface,
                u,
                depth + 1,
                geometry_budget,
            )?
        {
            if let Some(radial) = FiniteVector3::new(Vector3::new(
                point.x - center.x,
                point.y - center.y,
                point.z - center.z,
            ))
            .and_then(FiniteVector3::unit_nonzero)
            {
                let alpha = signed_angle(first, second, tangent);
                if alpha.is_finite() && alpha.abs() > EPS_BLEND_EXACT_GEOMETRY {
                    let theta = signed_angle(first, radial, tangent);
                    let mut candidates = [None; 5];
                    for (slot, turn) in (-2..=2).enumerate() {
                        let raw_v = (theta + f64::from(turn) * std::f64::consts::TAU) / alpha;
                        let Some(v) = section_domain.clamp_near_boundary(raw_v) else {
                            continue;
                        };
                        let Some(candidate) = blend_surface_point_inner_with_index_and_budget(
                            index,
                            surface,
                            u,
                            v,
                            depth + 1,
                            geometry_budget,
                        )?
                        else {
                            continue;
                        };
                        let branch_distance = seed.map_or(v.abs(), |seed| (v - seed.v).abs());
                        candidates[slot] = Some((
                            Point2::new(u, v),
                            Point3::distance(candidate, point),
                            branch_distance,
                        ));
                    }
                    candidates
                        .into_iter()
                        .flatten()
                        .min_by(|first, second| {
                            if (first.1 - second.1).abs() <= EPS_BLEND_EXACT_GEOMETRY {
                                first.2.total_cmp(&second.2)
                            } else {
                                first.1.total_cmp(&second.1)
                            }
                        })
                        .map(|(parameters, distance, _)| (parameters, distance))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some((initial, angular_distance)) = angular {
        // The angular candidate has already been evaluated against the query
        // point. A fit-qualified candidate is a complete inverse result; do
        // not spend the recursive Newton solve to rediscover the same proof.
        if section_domain.contains(initial.v)
            && fit_tolerance.is_some_and(|tolerance| angular_distance <= tolerance)
        {
            return Ok(Some(initial));
        }
        let parameters = refine_blend_surface_parameters_with_section_domain_and_budget(
            index,
            surface,
            point,
            initial,
            depth + 1,
            section_domain,
            geometry_budget,
        )?
        .unwrap_or(initial);
        if section_domain.contains(parameters.v) {
            if let Some(candidate) = blend_surface_point_inner_with_index_and_budget(
                index,
                surface,
                parameters.u,
                parameters.v,
                depth + 1,
                geometry_budget,
            )? {
                let distance = Point3::distance(candidate, point);
                if fit_tolerance.is_none_or(|tolerance| distance <= tolerance) {
                    return Ok(Some(parameters));
                }
            }
        }
    }
    let initial = match grid {
        #[cfg(test)]
        BlendParameterGrid::Build => coarse_blend_surface_parameters_with_index_and_budget(
            index,
            surface,
            point,
            depth + 1,
            geometry_budget,
        )?,
        BlendParameterGrid::Disabled => None,
        BlendParameterGrid::Provided(grid) => grid.closest_parameters(point),
    };
    if let Some(initial) = initial {
        let parameters = refine_blend_surface_parameters_with_section_domain_and_budget(
            index,
            surface,
            point,
            initial,
            depth + 1,
            section_domain,
            geometry_budget,
        )?
        .unwrap_or(initial);
        if section_domain.contains(parameters.v) {
            let Some(candidate) = blend_surface_point_inner_with_index_and_budget(
                index,
                surface,
                parameters.u,
                parameters.v,
                depth + 1,
                geometry_budget,
            )?
            else {
                return Ok(None);
            };
            let distance = Point3::distance(candidate, point);
            if fit_tolerance.is_none_or(|tolerance| distance <= tolerance) {
                return Ok(Some(parameters));
            }
        }
    }
    if let Some(fit_tolerance) = fit_tolerance {
        let boundary_parameters = [
            blend_boundary_parameter_with_index_and_budget(
                index,
                &BlendBoundaryFit {
                    surface,
                    point,
                    boundary: 0,
                    seed: seed.map(|seed| seed.u),
                    fit_tolerance,
                    depth: depth + 1,
                },
                geometry_budget,
            )?,
            blend_boundary_parameter_with_index_and_budget(
                index,
                &BlendBoundaryFit {
                    surface,
                    point,
                    boundary: 1,
                    seed: seed.map(|seed| seed.u),
                    fit_tolerance,
                    depth: depth + 1,
                },
                geometry_budget,
            )?,
        ];
        if let Some((parameter, boundary)) = match boundary_parameters {
            [Some(parameter), None] => Some((parameter, 0usize)),
            [None, Some(parameter)] => Some((parameter, 1usize)),
            _ => None,
        } {
            return Ok(Some(Point2::new(parameter, {
                let Some(value) = cadmpeg_core::convert::f64_from_index(boundary) else {
                    return Ok(None);
                };
                value
            })));
        }
    }
    Ok(None)
}

#[cfg(test)]
pub(super) fn coarse_blend_surface_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    coarse_blend_surface_parameters_with_index_and_budget(
        &index,
        surface,
        point,
        depth,
        &geometry_budget,
    )
}

#[cfg(test)]
fn coarse_blend_surface_parameters_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let Some(grid) =
        blend_surface_parameter_grid_with_index_and_budget(index, surface, depth, geometry_budget)?
    else {
        return Ok(None);
    };
    Ok(grid.closest_parameters(point))
}

pub(super) fn blend_surface_parameter_grid_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<BlendSurfaceGrid>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition { spine, .. }) =
        blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let Some(curve) = index.curves(spine.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
        return Ok(None);
    };
    let Some(degree) = usize::try_from(nurbs.degree()).ok() else {
        return Ok(None);
    };
    let count = nurbs.pole_rows().count();
    let (Some(lower), Some(upper)) = (nurbs.knots().get(degree), nurbs.knots().get(count)) else {
        return Ok(None);
    };
    let domain = [*lower, *upper];
    if !domain.into_iter().all(f64::is_finite) || domain[0] >= domain[1] {
        return Ok(None);
    }
    let mut grid = BlendSurfaceGrid {
        samples: [(Point2::new(0.0, 0.0), Point3::new(0.0, 0.0, 0.0)); BLEND_GRID_SAMPLES],
        len: 0,
    };
    for u_index in 0..=8 {
        let Some(u) = cadmpeg_ir::math::interpolate(domain[0], domain[1], f64::from(u_index) / 8.0)
        else {
            return Ok(None);
        };
        let u = u.get();
        let frame = blend_surface_frame_with_index_and_budget(
            index,
            surface,
            u,
            depth + 1,
            geometry_budget,
        )?;
        for v_index in 0..=4 {
            let parameters = Point2::new(u, f64::from(v_index) / 4.0);
            let point = match v_index {
                0 => blend_boundary_point_with_index_and_budget(
                    index,
                    surface,
                    u,
                    0,
                    depth + 1,
                    geometry_budget,
                )?,
                4 => blend_boundary_point_with_index_and_budget(
                    index,
                    surface,
                    u,
                    1,
                    depth + 1,
                    geometry_budget,
                )?,
                _ => frame.map(|frame| blend_surface_point_from_frame(frame, parameters.v)),
            };
            let Some(point) = point else {
                continue;
            };
            // The nine spine and five section samples fill at most every slot.
            grid.samples[grid.len] = (parameters, point);
            grid.len += 1;
        }
    }
    Ok((grid.len != 0).then_some(grid))
}

pub(super) fn blend_surface_parameters_from_grid_for_fit_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    fit_tolerance: f64,
    grid: &BlendSurfaceGrid,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    blend_surface_parameters_from_grid_for_fit_with_section_domain_and_budget(
        index,
        surface,
        point,
        fit_tolerance,
        grid,
        BlendSectionDomain::Canonical,
        geometry_budget,
    )
}

pub(super) fn blend_surface_parameters_from_grid_for_fit_with_source_continuation_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    fit_tolerance: f64,
    grid: &BlendSurfaceGrid,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    blend_surface_parameters_from_grid_for_fit_with_section_domain_and_budget(
        index,
        surface,
        point,
        fit_tolerance,
        grid,
        BlendSectionDomain::SourceContinuation,
        geometry_budget,
    )
}

fn blend_surface_parameters_from_grid_for_fit_with_section_domain_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    fit_tolerance: f64,
    grid: &BlendSurfaceGrid,
    section_domain: BlendSectionDomain,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let Some(initial) = grid.closest_parameters(point) else {
        return Ok(None);
    };
    let parameters = refine_blend_surface_parameters_with_section_domain_and_budget(
        index,
        surface,
        point,
        initial,
        0,
        section_domain,
        geometry_budget,
    )?
    .unwrap_or(initial);
    if !section_domain.contains(parameters.v) {
        return Ok(None);
    }
    let Some(candidate) = blend_surface_point_inner_with_index_and_budget(
        index,
        surface,
        parameters.u,
        parameters.v,
        0,
        geometry_budget,
    )?
    else {
        return Ok(None);
    };
    Ok((Point3::distance(candidate, point) <= fit_tolerance).then_some(parameters))
}

#[cfg(test)]
pub(super) fn refine_blend_surface_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    parameters: Point2,
    depth: usize,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    refine_blend_surface_parameters_with_section_domain_and_budget(
        &index,
        surface,
        point,
        parameters,
        depth,
        BlendSectionDomain::Canonical,
        &geometry_budget,
    )
}

fn refine_blend_surface_parameters_with_section_domain_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    mut parameters: Point2,
    depth: usize,
    section_domain: BlendSectionDomain,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition { spine, .. }) =
        blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let u_domain = index
        .curves(spine.as_str(), geometry_budget.charges)?
        .and_then(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let degree = usize::try_from(nurbs.degree()).ok()?;
                let count = nurbs.pole_rows().count();
                Some([*nurbs.knots().get(degree)?, *nurbs.knots().get(count)?])
            }
            _ => None,
        });
    if let Some(domain) = u_domain {
        parameters.u = parameters.u.clamp(domain[0], domain[1]);
    }
    let [section_lower, section_upper] = section_domain.bounds();
    parameters.v = parameters.v.clamp(section_lower, section_upper);
    let squared_distance = |candidate: Point3| {
        (candidate.x - point.x).powi(2)
            + (candidate.y - point.y).powi(2)
            + (candidate.z - point.z).powi(2)
    };
    for _ in 0..16 {
        let Some(position) = blend_surface_point_inner_with_index_and_budget(
            index,
            surface,
            parameters.u,
            parameters.v,
            depth + 1,
            geometry_budget,
        )?
        else {
            return Ok(None);
        };
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let current_distance = squared_distance(position);
        let u_step = parameter_derivative_step(parameters.u, u_domain);
        let derivative =
            |step: f64| -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
                let mut before = parameters;
                let mut after = parameters;
                before.u -= step;
                after.u += step;
                if let Some(domain) = u_domain {
                    before.u = before.u.clamp(domain[0], domain[1]);
                    after.u = after.u.clamp(domain[0], domain[1]);
                }
                let width = after.u - before.u;
                if !width.is_finite() || width == 0.0 {
                    return Ok(None);
                }
                let Some(first) = blend_surface_point_inner_with_index_and_budget(
                    index,
                    surface,
                    before.u,
                    before.v,
                    depth + 1,
                    geometry_budget,
                )?
                else {
                    return Ok(None);
                };
                let Some(second) = blend_surface_point_inner_with_index_and_budget(
                    index,
                    surface,
                    after.u,
                    after.v,
                    depth + 1,
                    geometry_budget,
                )?
                else {
                    return Ok(None);
                };
                Ok(Some(Vector3::new(
                    (second.x - first.x) / width,
                    (second.y - first.y) / width,
                    (second.z - first.z) / width,
                )))
            };
        let du = blend_surface_u_derivative_with_index_and_budget(
            index,
            surface,
            parameters.u,
            parameters.v,
            depth + 1,
            geometry_budget,
        )?;
        let du = if du.is_some() {
            du
        } else {
            derivative(u_step)?
        };
        let Some(du) = du else {
            return Ok(None);
        };
        let Some((_, tangent, first, second, radius)) = blend_surface_frame_with_index_and_budget(
            index,
            surface,
            parameters.u,
            depth + 1,
            geometry_budget,
        )?
        else {
            return Ok(None);
        };
        let alpha = signed_angle(first, second, tangent);
        let radial = rodrigues_rotate(first, tangent, parameters.v * alpha);
        let section_tangent = tangent.cross(radial);
        let dv = Vector3::new(
            radius * alpha * section_tangent.x,
            radius * alpha * section_tangent.y,
            radius * alpha * section_tangent.z,
        );
        let Some((step_u, step_v)) = FiniteVector3::new(du)
            .zip(FiniteVector3::new(dv))
            .and_then(|(du, dv)| least_squares_step(du, dv, residual))
        else {
            break;
        };
        let (step_u, step_v) = (step_u.get(), step_v.get());
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..8 {
            let mut candidate =
                Point2::new(parameters.u - scale * step_u, parameters.v - scale * step_v);
            if let Some(domain) = u_domain {
                candidate.u = candidate.u.clamp(domain[0], domain[1]);
            }
            candidate.v = candidate.v.clamp(section_lower, section_upper);
            if let Some(position) = blend_surface_point_inner_with_index_and_budget(
                index,
                surface,
                candidate.u,
                candidate.v,
                depth + 1,
                geometry_budget,
            )? {
                if squared_distance(position) < current_distance {
                    accepted = Some(candidate);
                    break;
                }
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        let converged = (candidate.u - parameters.u).abs()
            <= EPS_BLEND_EXACT_GEOMETRY * (1.0 + parameters.u.abs())
            && (candidate.v - parameters.v).abs()
                <= EPS_BLEND_EXACT_GEOMETRY * (1.0 + parameters.v.abs());
        parameters = candidate;
        if converged {
            break;
        }
    }
    Ok(Some(parameters))
}

#[cfg(test)]
pub(super) fn blend_surface_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    blend_surface_point_inner(ctx, ir, surface, u, v, 0)
}

#[cfg(test)]
fn blend_surface_point_inner(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    blend_surface_point_inner_with_index_and_budget(&index, surface, u, v, depth, &geometry_budget)
}

pub(super) fn blend_surface_point_inner_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    if v.to_bits() == 0.0f64.to_bits() {
        if let Some(point) = blend_boundary_point_with_index_and_budget(
            index,
            surface,
            u,
            0,
            depth + 1,
            geometry_budget,
        )? {
            return Ok(Some(point));
        }
    }
    if v.to_bits() == 1.0f64.to_bits() {
        if let Some(point) = blend_boundary_point_with_index_and_budget(
            index,
            surface,
            u,
            1,
            depth + 1,
            geometry_budget,
        )? {
            return Ok(Some(point));
        }
    }
    Ok(
        blend_surface_frame_with_index_and_budget(index, surface, u, depth + 1, geometry_budget)?
            .map(|frame| blend_surface_point_from_frame(frame, v)),
    )
}

type BlendSurfaceFrame = (Point3, Vector3, Vector3, Vector3, f64);

const MAX_BLEND_SURFACE_FRAME_CACHE_ENTRIES: usize = 512;
const MAX_BLEND_BOUNDARY_POINT_CACHE_ENTRIES: usize = 2_048;

struct BlendSurfaceFrameCacheEntry {
    surface: String,
    parameter_bits: u64,
    allow_offset_contact: bool,
    frame: BlendSurfaceFrame,
}

struct BlendBoundaryPointCacheEntry {
    surface: String,
    parameter_bits: u64,
    boundary: usize,
    point: Point3,
}

/// Bounded certificates for deterministic blend-geometry evaluations.
///
/// Entries belong to one [`GeometryWorkBudget`] and are valid only while its
/// model index is unchanged. Failed evaluations are not retained because a
/// later contact seed or route may produce a valid witness.
pub(super) struct BlendSurfaceFrameCache {
    entries: VecDeque<BlendSurfaceFrameCacheEntry>,
    boundary_points: VecDeque<BlendBoundaryPointCacheEntry>,
}

impl Default for BlendSurfaceFrameCache {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_BLEND_SURFACE_FRAME_CACHE_ENTRIES),
            boundary_points: VecDeque::with_capacity(MAX_BLEND_BOUNDARY_POINT_CACHE_ENTRIES),
        }
    }
}

// Each cache holds at most its fixed entry bound, so a scan visits a bounded
// number of entries. An entry's fixed fields are tested before its identity
// text, and only an identity comparison is input-sized work.
impl BlendSurfaceFrameCache {
    fn position(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        surface: &SurfaceId,
        parameter: f64,
        allow_offset_contact: bool,
    ) -> Result<Option<usize>, cadmpeg_core::decode::ResourceLimit> {
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.parameter_bits == parameter.to_bits()
                && entry.allow_offset_contact == allow_offset_contact
                && same_text(
                    ctx,
                    &entry.surface,
                    surface.as_str(),
                    "nx blend frame cache lookup",
                )?
            {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    fn get(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        surface: &SurfaceId,
        parameter: f64,
        allow_offset_contact: bool,
    ) -> Result<Option<BlendSurfaceFrame>, cadmpeg_core::decode::ResourceLimit> {
        Ok(self
            .position(ctx, surface, parameter, allow_offset_contact)?
            .and_then(|index| self.entries.get(index))
            .map(|entry| entry.frame))
    }

    fn remember(
        &mut self,
        surface: &SurfaceId,
        parameter: f64,
        allow_offset_contact: bool,
        frame: BlendSurfaceFrame,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        if let Some(entry) = self
            .position(
                geometry_budget.charges,
                surface,
                parameter,
                allow_offset_contact,
            )?
            .and_then(|index| self.entries.get_mut(index))
        {
            entry.frame = frame;
            return Ok(());
        }
        let surface = geometry_budget
            .charges
            .copy_retained_text_limit(surface.as_str(), "nx blend frame cache identity")?;
        geometry_budget.charges.charge_collection_items_limit(
            cadmpeg_core::decode::u64_from_index(1),
            "nx blend frame cache entries",
        )?;
        if self.entries.len() == MAX_BLEND_SURFACE_FRAME_CACHE_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(BlendSurfaceFrameCacheEntry {
            surface,
            parameter_bits: parameter.to_bits(),
            allow_offset_contact,
            frame,
        });
        Ok(())
    }

    fn boundary_position(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        surface: &SurfaceId,
        parameter: f64,
        boundary: usize,
    ) -> Result<Option<usize>, cadmpeg_core::decode::ResourceLimit> {
        for (index, entry) in self.boundary_points.iter().enumerate() {
            if entry.parameter_bits == parameter.to_bits()
                && entry.boundary == boundary
                && same_text(
                    ctx,
                    &entry.surface,
                    surface.as_str(),
                    "nx blend boundary cache lookup",
                )?
            {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    fn get_boundary_point(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        surface: &SurfaceId,
        parameter: f64,
        boundary: usize,
    ) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
        Ok(self
            .boundary_position(ctx, surface, parameter, boundary)?
            .and_then(|index| self.boundary_points.get(index))
            .map(|entry| entry.point))
    }

    fn remember_boundary_point(
        &mut self,
        surface: &SurfaceId,
        parameter: f64,
        boundary: usize,
        point: Point3,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        if let Some(entry) = self
            .boundary_position(geometry_budget.charges, surface, parameter, boundary)?
            .and_then(|index| self.boundary_points.get_mut(index))
        {
            entry.point = point;
            return Ok(());
        }
        let surface = geometry_budget
            .charges
            .copy_retained_text_limit(surface.as_str(), "nx blend boundary cache identity")?;
        geometry_budget.charges.charge_collection_items_limit(
            cadmpeg_core::decode::u64_from_index(1),
            "nx blend boundary cache entries",
        )?;
        if self.boundary_points.len() == MAX_BLEND_BOUNDARY_POINT_CACHE_ENTRIES {
            self.boundary_points.pop_front();
        }
        self.boundary_points
            .push_back(BlendBoundaryPointCacheEntry {
                surface,
                parameter_bits: parameter.to_bits(),
                boundary,
                point,
            });
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.boundary_points.clear();
    }
}

const MAX_BLEND_CONTACT_SEEDS: usize = 8;

struct BlendContactSeed<'k> {
    support: &'k SurfaceId,
    spine: &'k CurveId,
    parameter: f64,
    offset_surface: &'k SurfaceId,
    parameters: Point2,
}

/// Bounded chart seeds for one continuous pcurve transfer.
///
/// Seeds are scoped by the target support, its spine, and the offset carrier.
/// Keeping a small nearest-parameter set makes adaptive endpoint and midpoint
/// sampling local without allowing a model-wide cache to select a branch from
/// an unrelated intersection. A seed borrows its chart identities for the
/// cache's lifetime, so the cache holds no identity text of its own.
#[derive(Default)]
pub(super) struct BlendContactSeedCache<'k> {
    entries: Vec<BlendContactSeed<'k>>,
}

impl<'k> BlendContactSeedCache<'k> {
    /// Whether `seed` belongs to the chart of `support`, `spine` and
    /// `offset_surface`.
    fn same_chart(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        seed: &BlendContactSeed<'_>,
        support: &SurfaceId,
        spine: &CurveId,
        offset_surface: &SurfaceId,
    ) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
        const OPERATION: &str = "nx blend contact seed lookup";
        Ok(
            same_text(ctx, seed.support.as_str(), support.as_str(), OPERATION)?
                && same_text(ctx, seed.spine.as_str(), spine.as_str(), OPERATION)?
                && same_text(
                    ctx,
                    seed.offset_surface.as_str(),
                    offset_surface.as_str(),
                    OPERATION,
                )?,
        )
    }

    /// The parameters of the chart seed nearest `parameter`; the first of
    /// equally near seeds.
    fn seed_for(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        support: &SurfaceId,
        spine: &CurveId,
        parameter: f64,
        offset_surface: &SurfaceId,
    ) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
        let mut nearest: Option<&BlendContactSeed<'_>> = None;
        for seed in &self.entries {
            if !seed.parameter.is_finite()
                || !Self::same_chart(ctx, seed, support, spine, offset_surface)?
            {
                continue;
            }
            let closer = nearest.is_none_or(|nearest| {
                (seed.parameter - parameter)
                    .abs()
                    .total_cmp(&(nearest.parameter - parameter).abs())
                    .is_lt()
            });
            if closer {
                nearest = Some(seed);
            }
        }
        Ok(nearest.map(|seed| seed.parameters))
    }

    /// Record the chart parameters at `parameter`. A seed already recorded
    /// at the same parameter is updated in place; a full cache replaces the
    /// last of its seeds farthest from `parameter`.
    fn remember(
        &mut self,
        (support, spine, offset_surface): (&'k SurfaceId, &'k CurveId, &'k SurfaceId),
        parameter: f64,
        parameters: Point2,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        let ctx = geometry_budget.charges;
        for existing in &mut self.entries {
            if existing.parameter.to_bits() == parameter.to_bits()
                && Self::same_chart(ctx, existing, support, spine, offset_surface)?
            {
                existing.parameters = parameters;
                return Ok(());
            }
        }
        let seed = BlendContactSeed {
            support,
            spine,
            parameter,
            offset_surface,
            parameters,
        };
        if self.entries.len() < MAX_BLEND_CONTACT_SEEDS {
            // The cache holds at most its fixed seed bound.
            self.entries.push(seed);
            return Ok(());
        }
        let mut farthest: Option<(usize, f64)> = None;
        for (index, existing) in self.entries.iter().enumerate() {
            let distance = (existing.parameter - parameter).abs();
            if farthest.is_none_or(|(_, farthest)| !distance.total_cmp(&farthest).is_lt()) {
                farthest = Some((index, distance));
            }
        }
        if let Some((index, _)) = farthest {
            self.entries[index] = seed;
        }
        Ok(())
    }
}

fn blend_surface_point_from_frame(
    (center, tangent, first, second, radius): BlendSurfaceFrame,
    v: f64,
) -> Point3 {
    let alpha = signed_angle(first, second, tangent);
    let radial = rodrigues_rotate(first, tangent, v * alpha);
    Point3::new(
        center.x + radius * radial.x,
        center.y + radius * radial.y,
        center.z + radius * radial.z,
    )
}

#[cfg(test)]
pub(super) fn blend_surface_u_derivative(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    blend_surface_u_derivative_with_index_and_budget(&index, surface, u, v, depth, &geometry_budget)
}

fn blend_surface_u_derivative_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition {
        supports,
        spine,
        radius,
        ..
    }) = blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let Some(carrier) = index.curves(spine.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    let Some(center) = cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
            .within_work_slice(geometry_budget, |admission| {
                cadmpeg_ir::eval::decode::curve_point(admission, &carrier.geometry, u)
            }),
    )?
    else {
        return Ok(None);
    };
    let Some(velocity) = cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
            .within_work_slice(geometry_budget, |admission| {
                cadmpeg_ir::eval::decode::curve_tangent(admission, &carrier.geometry, u)
            }),
    )?
    else {
        return Ok(None);
    };
    let Some(acceleration) = cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
            .within_work_slice(geometry_budget, |admission| {
                cadmpeg_ir::eval::curve_second_derivative(admission, &carrier.geometry, u)
            }),
    )?
    else {
        return Ok(None);
    };
    let speed = velocity.norm();
    if !speed.is_finite() || speed == 0.0 {
        return Ok(None);
    }
    let tangent = Vector3::new(velocity.x / speed, velocity.y / speed, velocity.z / speed);
    let tangential_acceleration = tangent.dot(acceleration.get());
    let tangent_derivative = Vector3::new(
        (acceleration.x - tangential_acceleration * tangent.x) / speed,
        (acceleration.y - tangential_acceleration * tangent.y) / speed,
        (acceleration.z - tangential_acceleration * tangent.z) / speed,
    );
    let contact_context = BlendContactDerivativeContext {
        index,
        spine,
        parameter: u,
        center: center.get(),
        center_derivative: velocity.get(),
        radius,
        depth: depth + 1,
    };
    let Some((first, first_derivative)) =
        contact_context.direction_derivative(supports[0], geometry_budget)?
    else {
        return Ok(None);
    };
    let Some((second, second_derivative)) =
        contact_context.direction_derivative(supports[1], geometry_budget)?
    else {
        return Ok(None);
    };

    let cross = first.cross(second);
    let cosine = first.dot(second);
    let sine = cross.dot(tangent);
    let cosine_derivative = first_derivative.dot(second) + first.dot(second_derivative);
    let cross_derivative = first_derivative.cross(second) + first.cross(second_derivative);
    let sine_derivative = cross_derivative.dot(tangent) + cross.dot(tangent_derivative);
    let angle_denominator = cosine * cosine + sine * sine;
    if !angle_denominator.is_finite() || angle_denominator == 0.0 {
        return Ok(None);
    }
    let alpha = sine.atan2(cosine);
    let alpha_derivative =
        (cosine * sine_derivative - sine * cosine_derivative) / angle_denominator;
    let theta = v * alpha;
    let theta_derivative = v * alpha_derivative;
    let theta_cosine = theta.cos();
    let theta_sine = theta.sin();
    let tangent_cross_first = tangent.cross(first);
    let tangent_cross_first_derivative =
        tangent_derivative.cross(first) + tangent.cross(first_derivative);
    let tangent_dot_first = tangent.dot(first);
    let tangent_dot_first_derivative =
        tangent_derivative.dot(first) + tangent.dot(first_derivative);
    let radial_component = |first: f64,
                            first_derivative: f64,
                            tangent_cross_first: f64,
                            tangent_cross_first_derivative: f64,
                            tangent: f64,
                            tangent_derivative: f64| {
        first_derivative * theta_cosine - first * theta_sine * theta_derivative
            + tangent_cross_first_derivative * theta_sine
            + tangent_cross_first * theta_cosine * theta_derivative
            + tangent_derivative * tangent_dot_first * (1.0 - theta_cosine)
            + tangent * tangent_dot_first_derivative * (1.0 - theta_cosine)
            + tangent * tangent_dot_first * theta_sine * theta_derivative
    };
    let radial_derivative = Vector3::new(
        radial_component(
            first.x,
            first_derivative.x,
            tangent_cross_first.x,
            tangent_cross_first_derivative.x,
            tangent.x,
            tangent_derivative.x,
        ),
        radial_component(
            first.y,
            first_derivative.y,
            tangent_cross_first.y,
            tangent_cross_first_derivative.y,
            tangent.y,
            tangent_derivative.y,
        ),
        radial_component(
            first.z,
            first_derivative.z,
            tangent_cross_first.z,
            tangent_cross_first_derivative.z,
            tangent.z,
            tangent_derivative.z,
        ),
    );
    Ok(Some(Vector3::new(
        velocity.x + radius * radial_derivative.x,
        velocity.y + radius * radial_derivative.y,
        velocity.z + radius * radial_derivative.z,
    )))
}

struct BlendContactDerivativeContext<'a> {
    index: &'a cadmpeg_ir::index::ModelIndex<'a>,
    spine: &'a CurveId,
    parameter: f64,
    center: Point3,
    center_derivative: Vector3,
    radius: f64,
    depth: usize,
}

impl BlendContactDerivativeContext<'_> {
    fn direction_derivative(
        &self,
        support: &SurfaceId,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<Option<(Vector3, Vector3)>, cadmpeg_core::decode::ResourceLimit> {
        if self.depth >= 32 {
            return Ok(None);
        }
        let Some(pcurve) = spine_contact_pcurve_with_index(
            self.index,
            support,
            self.spine,
            self.radius,
            self.depth + 1,
            geometry_budget.charges,
        )?
        else {
            return Ok(None);
        };
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            geometry_budget.charges,
            pcurve,
            self.parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(uv_derivative) = cadmpeg_ir::eval::finite_or_refusal(pcurve_tangent(
            geometry_budget.charges,
            pcurve,
            self.parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(support) = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
                .within_work_slice(geometry_budget, |admission| {
                    model_surface_partials_by_id(admission, self.index, support, uv.u, uv.v)
                }),
        )?
        else {
            return Ok(None);
        };
        let support = support.into_raw();
        let contact_derivative = Vector3::new(
            support.du.x * uv_derivative.u + support.dv.x * uv_derivative.v,
            support.du.y * uv_derivative.u + support.dv.y * uv_derivative.v,
            support.du.z * uv_derivative.u + support.dv.z * uv_derivative.v,
        );
        let offset = Vector3::new(
            support.point.x - self.center.x,
            support.point.y - self.center.y,
            support.point.z - self.center.z,
        );
        let magnitude = offset.norm();
        if !magnitude.is_finite() || magnitude == 0.0 {
            return Ok(None);
        }
        let direction = Vector3::new(
            offset.x / magnitude,
            offset.y / magnitude,
            offset.z / magnitude,
        );
        let offset_derivative = Vector3::new(
            contact_derivative.x - self.center_derivative.x,
            contact_derivative.y - self.center_derivative.y,
            contact_derivative.z - self.center_derivative.z,
        );
        let radial_derivative = direction.dot(offset_derivative);
        let direction_derivative = Vector3::new(
            (offset_derivative.x - radial_derivative * direction.x) / magnitude,
            (offset_derivative.y - radial_derivative * direction.y) / magnitude,
            (offset_derivative.z - radial_derivative * direction.z) / magnitude,
        );
        Ok(Some((direction, direction_derivative)))
    }
}

fn blend_surface_frame_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<BlendSurfaceFrame>, cadmpeg_core::decode::ResourceLimit> {
    let mut contact_seeds = BlendContactSeedCache::default();
    blend_surface_frame_with_index_and_budget_and_options(
        index,
        surface,
        u,
        depth,
        false,
        &mut contact_seeds,
        geometry_budget,
    )
}

fn blend_surface_frame_with_index_and_budget_and_options<'k>(
    index: &'k cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    depth: usize,
    allow_offset_contact: bool,
    contact_seeds: &mut BlendContactSeedCache<'k>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<BlendSurfaceFrame>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    if !allow_offset_contact {
        let cached = geometry_budget.blend_frame_cache().borrow().get(
            geometry_budget.charges,
            surface,
            u,
            allow_offset_contact,
        )?;
        if let Some(frame) = cached {
            if !geometry_budget.charge() {
                return geometry_budget.resource_refusal().map_or(Ok(None), Err);
            }
            return Ok(Some(frame));
        }
    }
    let frame = (|| -> Result<Option<BlendSurfaceFrame>, cadmpeg_core::decode::ResourceLimit> {
        let Some(CircularBlendDefinition {
            supports,
            spine,
            radius,
            ..
        }) = blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
        else {
            return Ok(None);
        };
        let Some(center) =
            model_curve_point_with_index_and_budget(index, spine, u, geometry_budget)?
        else {
            return Ok(None);
        };
        let Some(tangent) =
            model_curve_tangent_with_index_and_budget(index, spine, u, geometry_budget)?
        else {
            return Ok(None);
        };
        let first = spine_contact_direction_with_index_and_budget_and_options(
            index,
            &SpineContactLocation {
                support: supports[0],
                spine,
                parameter: u,
                radius,
            },
            center,
            depth + 1,
            allow_offset_contact,
            contact_seeds,
            geometry_budget,
        )?;
        let first = if first.is_some() {
            first
        } else {
            surface_contact_direction_with_index_and_budget(
                index,
                supports[0],
                center,
                radius,
                depth + 1,
                geometry_budget,
            )?
        };
        let Some(first) = first else {
            return Ok(None);
        };
        let second = spine_contact_direction_with_index_and_budget_and_options(
            index,
            &SpineContactLocation {
                support: supports[1],
                spine,
                parameter: u,
                radius,
            },
            center,
            depth + 1,
            allow_offset_contact,
            contact_seeds,
            geometry_budget,
        )?;
        let second = if second.is_some() {
            second
        } else {
            surface_contact_direction_with_index_and_budget(
                index,
                supports[1],
                center,
                radius,
                depth + 1,
                geometry_budget,
            )?
        };
        Ok(second.map(|second| (center, tangent, first, second, radius)))
    })()?;
    if !allow_offset_contact {
        if let Some(frame) = frame {
            geometry_budget.blend_frame_cache().borrow_mut().remember(
                surface,
                u,
                allow_offset_contact,
                frame,
                geometry_budget,
            )?;
        }
    }
    Ok(frame)
}

// Keep the frame inputs, recursion depth, contact policy, seed cache, and
// caller-owned work slice explicit at this geometry boundary.

struct SpineContactLocation<'inputs> {
    support: &'inputs SurfaceId,
    spine: &'inputs CurveId,
    parameter: f64,
    radius: f64,
}

fn spine_contact_direction_with_index_and_budget_and_options<'k>(
    index: &'k cadmpeg_ir::index::ModelIndex<'_>,
    spine_contact_location: &SpineContactLocation<'k>,
    center: Point3,
    depth: usize,
    allow_offset_contact: bool,
    contact_seeds: &mut BlendContactSeedCache<'k>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    let Some(contact) = spine_contact_point_with_index_and_budget_and_options(
        index,
        spine_contact_location,
        depth + 1,
        allow_offset_contact,
        contact_seeds,
        geometry_budget,
    )?
    else {
        return Ok(None);
    };
    Ok(FiniteVector3::new(Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    ))
    .and_then(FiniteVector3::unit_nonzero))
}

fn blend_boundary_point_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    parameter: f64,
    boundary: usize,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    let cached = geometry_budget
        .blend_frame_cache()
        .borrow()
        .get_boundary_point(geometry_budget.charges, surface, parameter, boundary)?;
    if let Some(point) = cached {
        if !geometry_budget.charge() {
            return geometry_budget.resource_refusal().map_or(Ok(None), Err);
        }
        return Ok(Some(point));
    }
    let Some(CircularBlendDefinition {
        supports,
        spine,
        radius,
        ..
    }) = blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let Some(support) = supports.get(boundary) else {
        return Ok(None);
    };
    let point = spine_contact_point_with_index_and_budget(
        index,
        support,
        spine,
        parameter,
        radius,
        depth + 1,
        geometry_budget,
    )?;
    if let Some(point) = point {
        geometry_budget
            .blend_frame_cache()
            .borrow_mut()
            .remember_boundary_point(surface, parameter, boundary, point, geometry_budget)?;
    }
    Ok(point)
}

// Boundary inversion carries the geometric query state and the shared work
// slice explicitly so nested certification cannot allocate a private budget.

struct BlendBoundaryFit<'inputs> {
    surface: &'inputs SurfaceId,
    point: Point3,
    boundary: usize,
    seed: Option<f64>,
    fit_tolerance: f64,
    depth: usize,
}

fn blend_boundary_parameter_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    blend_boundary_fit: &BlendBoundaryFit<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let &BlendBoundaryFit {
        surface,
        point,
        boundary,
        seed,
        fit_tolerance,
        depth,
    } = blend_boundary_fit;

    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition { spine, .. }) =
        blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    // A circular blend's u parameter is its spine parameter. Invert that
    // defining carrier directly, then certify the requested boundary point.
    let Some(parameter) =
        closest_spine_parameter_with_index_and_budget(index, spine, point, seed, geometry_budget)?
    else {
        return Ok(None);
    };
    Ok(blend_boundary_point_with_index_and_budget(
        index,
        surface,
        parameter,
        boundary,
        depth + 1,
        geometry_budget,
    )?
    .filter(|candidate| Point3::distance(*candidate, point) <= fit_tolerance)
    .map(|_| parameter))
}

#[derive(Clone, Copy)]
pub(super) struct BoundaryInverseTarget {
    pub(super) point: Point3,
    pub(super) seed: Option<Point2>,
    pub(super) tolerance: f64,
}

pub(super) fn blend_boundary_parameter_from_support_pcurve_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    blend: &SurfaceId,
    support: &SurfaceId,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    target: BoundaryInverseTarget,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let Some(carrier) = index.surfaces(support.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    let support_geometry = &carrier.geometry;
    blend_boundary_parameter_from_support_pcurve_with_geometry_and_budget(
        index,
        blend,
        &SupportCurveSample {
            support,
            support_geometry,
            support_pcurve,
            curve_parameter,
        },
        target,
        geometry_budget,
    )
}

struct SupportCurveSample<'inputs> {
    support: &'inputs SurfaceId,
    support_geometry: &'inputs SurfaceGeometry,
    support_pcurve: &'inputs PcurveGeometry,
    curve_parameter: f64,
}

fn blend_boundary_parameter_from_support_pcurve_with_geometry_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    blend: &SurfaceId,
    support_curve_sample: &SupportCurveSample<'_>,
    target: BoundaryInverseTarget,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let &SupportCurveSample {
        support,
        support_geometry,
        support_pcurve,
        curve_parameter,
    } = support_curve_sample;

    let Some(CircularBlendDefinition {
        supports,
        spine,
        radius,
        ..
    }) = blend_surface_definition_with_index(index, blend, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let mut selected = None;
    for (boundary, candidate) in supports.into_iter().enumerate() {
        if parameterization_equivalent_surfaces_with_index(
            index,
            candidate,
            support,
            geometry_budget.charges,
        )? && selected.replace(boundary).is_some()
        {
            return Ok(None);
        }
    }
    let Some(boundary) = selected else {
        return Ok(None);
    };
    let Some(contact_pcurve) =
        spine_contact_pcurve_with_index(index, support, spine, radius, 0, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    blend_boundary_parameter_from_contact_pcurve_with_geometry_and_budget(
        index,
        &ContactCurveSample {
            support,
            support_geometry,
            contact_pcurve,
            boundary,
            support_pcurve,
            curve_parameter,
        },
        target,
        geometry_budget,
    )
}

pub(super) struct ContactCurveSample<'inputs> {
    pub(super) support: &'inputs SurfaceId,
    pub(super) support_geometry: &'inputs SurfaceGeometry,
    pub(super) contact_pcurve: &'inputs PcurveGeometry,
    pub(super) boundary: usize,
    pub(super) support_pcurve: &'inputs PcurveGeometry,
    pub(super) curve_parameter: f64,
}

pub(super) fn blend_boundary_parameter_from_contact_pcurve_with_geometry_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    contact_curve_sample: &ContactCurveSample<'_>,
    target: BoundaryInverseTarget,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let &ContactCurveSample {
        support,
        support_geometry,
        contact_pcurve,
        boundary,
        support_pcurve,
        curve_parameter,
    } = contact_curve_sample;

    let Some(support_uv) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            geometry_budget.charges,
            support_pcurve,
            curve_parameter,
        ))?
    else {
        return Ok(None);
    };
    let seeded = match target.seed {
        Some(seed) => closest_pcurve_parameter_from_seed(
            geometry_budget.charges,
            contact_pcurve,
            support_uv.get(),
            seed.u,
        )?,
        None => None,
    };
    let parameter = match seeded {
        Some(parameter) => Some(parameter),
        None => closest_pcurve_parameter_from_coarse_grid(
            geometry_budget.charges,
            contact_pcurve,
            support_uv.get(),
        )?,
    };
    let Some(parameter) = parameter else {
        return Ok(None);
    };
    let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
        geometry_budget.charges,
        contact_pcurve,
        parameter,
    ))?
    else {
        return Ok(None);
    };
    let Some(candidate) = decoded_surface_point_with_geometry_and_budget(
        index,
        support,
        support_geometry,
        uv.u,
        uv.v,
        0,
        geometry_budget,
    )?
    else {
        return Ok(None);
    };
    Ok(
        (Point3::distance(candidate, target.point) <= target.tolerance).then_some(Point2::new(
            parameter,
            {
                let Some(value) = cadmpeg_core::convert::f64_from_index(boundary) else {
                    return Ok(None);
                };
                value
            },
        )),
    )
}

pub(super) struct SourcePcurveSample<'inputs> {
    pub(super) blend: &'inputs SurfaceId,
    pub(super) support: &'inputs SurfaceId,
    pub(super) source_pcurve: &'inputs PcurveGeometry,
    pub(super) curve_parameter: f64,
}

/// Transfer a source-chart sample from a blend boundary onto its declared
/// support chart.
///
/// Nested blend supports are inverted from the source sample directly.  For
/// analytic and offset supports, the serialized spine contact chart remains
/// the fast path, with a bounded 3D closest-point fallback.  Every result is
/// certified by reproducing the source sample on the target support.
pub(super) fn blend_support_parameter_from_source_pcurve_with_index_and_budget_and_seed_cache<
    'k,
>(
    index: &'k cadmpeg_ir::index::ModelIndex<'_>,
    source_pcurve_sample: &SourcePcurveSample<'_>,
    target: BoundaryInverseTarget,
    contact_seeds: &mut BlendContactSeedCache<'k>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let &SourcePcurveSample {
        blend,
        support,
        source_pcurve,
        curve_parameter,
    } = source_pcurve_sample;

    let Some(CircularBlendDefinition { supports, .. }) =
        blend_surface_definition_with_index(index, blend, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let mut matches = 0;
    for candidate in supports {
        if parameterization_equivalent_surfaces_with_index(
            index,
            candidate,
            support,
            geometry_budget.charges,
        )? {
            matches += 1;
        }
    }
    if matches != 1 {
        return Ok(None);
    }
    let Some(source_uv) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            geometry_budget.charges,
            source_pcurve,
            curve_parameter,
        ))?
    else {
        return Ok(None);
    };
    if !target.point.is_finite() || !target.tolerance.is_finite() || target.tolerance < 0.0 {
        return Ok(None);
    }

    // A nested blend support does not use the source blend's spine contact
    // chart.  Its completed boundary lane is a separate intersection whose
    // pcurve is carried by the source blend.  Invert the already evaluated
    // source sample on the declared support and require point reproduction;
    // the support declaration is the relation proof and the fit is the
    // geometric certificate.
    if blend_surface_definition_with_index(index, support, geometry_budget.charges)?.is_some() {
        if let Some(parameters) = blend_surface_parameters_from_point_with_index_and_budget(
            index,
            support,
            target.point,
            target.seed,
            target.tolerance,
            contact_seeds,
            geometry_budget,
        )? {
            return Ok(Some(parameters));
        }
    }

    let Some(CircularBlendDefinition { spine, radius, .. }) =
        blend_surface_definition_with_index(index, blend, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let Some(contact_pcurve) =
        spine_contact_pcurve_with_index(index, support, spine, radius, 0, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let certify = |parameter: f64| -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            geometry_budget.charges,
            contact_pcurve,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(candidate) = decoded_surface_point_inner_with_budget(
            index,
            support,
            uv.u,
            uv.v,
            0,
            geometry_budget,
        )?
        else {
            return Ok(None);
        };
        Ok((Point3::distance(candidate, target.point) <= target.tolerance).then_some(uv.get()))
    };
    if let Some(uv) = certify(source_uv.u)? {
        return Ok(Some(uv));
    }
    let Some(parameter) = closest_contact_pcurve_parameter_with_geometry_and_budget(
        index,
        support,
        contact_pcurve,
        target.point,
        Some(source_uv.u),
        geometry_budget,
    )?
    else {
        return Ok(None);
    };
    certify(parameter)
}

pub(super) fn blend_surface_parameters_from_point_with_index_and_budget<'k>(
    index: &'k cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    contact_seeds: &mut BlendContactSeedCache<'k>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    // A circular blend sample can lie on the finite continuation of either
    // rail.  Keep both chart domains bounded and require point reproduction
    // before admitting the recovered section parameter.
    let Some(CircularBlendDefinition { spine, radius, .. }) =
        blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let Some(parameter) = closest_spine_parameter_with_index_and_budget(
        index,
        spine,
        point,
        seed.map(|seed| seed.u),
        geometry_budget,
    )?
    else {
        return Ok(None);
    };
    let Some((center, tangent, first, second, _)) =
        blend_surface_frame_with_index_and_budget_and_options(
            index,
            surface,
            parameter,
            0,
            true,
            contact_seeds,
            geometry_budget,
        )?
    else {
        return Ok(None);
    };
    let Some(radial) = FiniteVector3::new(Vector3::new(
        point.x - center.x,
        point.y - center.y,
        point.z - center.z,
    ))
    .and_then(FiniteVector3::unit_nonzero) else {
        return Ok(None);
    };
    let alpha = signed_angle(first, second, tangent);
    if !alpha.is_finite() || alpha.abs() <= BLEND_INVERSE_MIN_SWEEP {
        return Ok(None);
    }
    let theta = signed_angle(first, radial, tangent);
    // Two section domains by five turns state at most ten candidates; the
    // first nearest one is kept.
    let mut best: Option<(Point2, f64, f64)> = None;
    for section_domain in [
        BlendSectionDomain::Canonical,
        BlendSectionDomain::SourceContinuation,
    ] {
        for turn in -2..=2 {
            let raw_v = (theta + f64::from(turn) * std::f64::consts::TAU) / alpha;
            let Some(v) = section_domain.clamp_near_boundary(raw_v) else {
                continue;
            };
            let candidate =
                blend_surface_point_from_frame((center, tangent, first, second, radius), v);
            let distance = Point3::distance(candidate, point);
            if distance.is_finite() && distance <= fit_tolerance {
                let candidate = (
                    Point2::new(parameter, v),
                    seed.map_or(v.abs(), |seed| (v - seed.v).abs()),
                    distance,
                );
                let nearer = best.is_none_or(|best| {
                    candidate
                        .2
                        .total_cmp(&best.2)
                        .then_with(|| candidate.1.total_cmp(&best.1))
                        .is_lt()
                });
                if nearer {
                    best = Some(candidate);
                }
            }
        }
    }
    Ok(best.map(|(parameters, _, _)| parameters))
}

fn closest_contact_pcurve_parameter_with_geometry_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    contact_pcurve: &PcurveGeometry,
    point: Point3,
    seed: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Some((domain, periodic)) = pcurve_domain(contact_pcurve) else {
        return Ok(None);
    };
    let normalize = |parameter: f64| {
        if periodic {
            canonical_periodic_parameter(domain, true, parameter)
        } else {
            parameter.clamp(domain[0], domain[1])
        }
    };
    let distance = |parameter: f64| -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            geometry_budget.charges,
            contact_pcurve,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(candidate) = decoded_surface_point_inner_with_budget(
            index,
            support,
            uv.u,
            uv.v,
            0,
            geometry_budget,
        )?
        else {
            return Ok(None);
        };
        let distance = candidate.distance(point);
        Ok(distance.is_finite().then_some(distance))
    };
    let mut best = None;
    for index in 0..=COARSE_CONTACT_PCURVE_SEARCH_INTERVALS {
        let Some(parameter) = cadmpeg_ir::math::interpolate(
            domain[0],
            domain[1],
            {
                let Some(value) = cadmpeg_core::convert::f64_from_index(index) else {
                    return Ok(None);
                };
                value
            } / {
                let Some(value) =
                    cadmpeg_core::convert::f64_from_index(COARSE_CONTACT_PCURVE_SEARCH_INTERVALS)
                else {
                    return Ok(None);
                };
                value
            },
        ) else {
            continue;
        };
        let parameter = parameter.get();
        if let Some(candidate_distance) = distance(parameter)? {
            if best.is_none_or(|(_, best_distance)| candidate_distance <= best_distance) {
                best = Some((parameter, candidate_distance));
            }
        }
    }
    if let Some(seed) = seed.filter(|seed| seed.is_finite()) {
        let parameter = normalize(seed);
        if let Some(candidate_distance) = distance(parameter)? {
            if best.is_none_or(|(_, best_distance)| candidate_distance <= best_distance) {
                best = Some((parameter, candidate_distance));
            }
        }
    }
    let Some((mut parameter, mut best_distance)) = best else {
        return Ok(None);
    };
    for _ in 0..LOCAL_CONTACT_PCURVE_SEARCH_STEPS {
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            geometry_budget.charges,
            contact_pcurve,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(uv_tangent) = cadmpeg_ir::eval::finite_or_refusal(pcurve_tangent(
            geometry_budget.charges,
            contact_pcurve,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(partials) = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
                .within_work_slice(geometry_budget, |admission| {
                    model_surface_partials_by_id(admission, index, support, uv.u, uv.v)
                }),
        )?
        else {
            return Ok(None);
        };
        let tangent = Vector3::new(
            partials.du.x * uv_tangent.u + partials.dv.x * uv_tangent.v,
            partials.du.y * uv_tangent.u + partials.dv.y * uv_tangent.v,
            partials.du.z * uv_tangent.u + partials.dv.z * uv_tangent.v,
        );
        let Some(unit) = FiniteVector3::new(tangent).and_then(FiniteVector3::unit_nonzero) else {
            break;
        };
        let scale = tangent.x.abs().max(tangent.y.abs()).max(tangent.z.abs());
        let scaled_length = (tangent.x / scale)
            .hypot(tangent.y / scale)
            .hypot(tangent.z / scale);
        let difference = Vector3::new(
            partials.point.x - point.x,
            partials.point.y - point.y,
            partials.point.z - point.z,
        );
        let residual_scale = difference
            .x
            .abs()
            .max(difference.y.abs())
            .max(difference.z.abs());
        if residual_scale == 0.0 {
            break;
        }
        let projection = Vector3::new(
            difference.x / residual_scale,
            difference.y / residual_scale,
            difference.z / residual_scale,
        )
        .dot(unit);
        let Some(step) = cadmpeg_ir::math::product_quotient(
            [residual_scale, projection],
            [scale, scaled_length],
        )
        .map(cadmpeg_ir::scalar::FiniteReal::get) else {
            break;
        };
        let previous = parameter;
        let next = normalize(parameter - step);
        let Some(distance) = distance(next)? else {
            break;
        };
        if distance >= best_distance {
            break;
        }
        best_distance = distance;
        parameter = next;
        if (next - previous).abs()
            <= (64.0 * f64::EPSILON * domain[1] - 64.0 * f64::EPSILON * domain[0]).abs()
        {
            break;
        }
    }
    Ok(Some(parameter))
}

const LOCAL_PCURVE_SEARCH_STEPS: usize = 12;
const COARSE_PCURVE_SEARCH_INTERVALS: usize = 16;
const LOCAL_CONTACT_PCURVE_SEARCH_STEPS: usize = 12;
const COARSE_CONTACT_PCURVE_SEARCH_INTERVALS: usize = 16;

fn pcurve_domain(pcurve: &PcurveGeometry) -> Option<([f64; 2], bool)> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let count = nurbs.pole_rows().count();
    let domain = [*nurbs.knots().get(degree)?, *nurbs.knots().get(count)?];
    (domain[0].is_finite() && domain[1].is_finite() && domain[0] < domain[1])
        .then_some((domain, nurbs.periodic()))
}

fn closest_pcurve_parameter_from_coarse_grid(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &PcurveGeometry,
    point: Point2,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    let Some((domain, _)) = pcurve_domain(pcurve) else {
        return Ok(None);
    };
    let mut closest = None;
    // A fixed count of samples; each evaluation charges its own work.
    for index in 0..=COARSE_PCURVE_SEARCH_INTERVALS {
        let Some(parameter) = cadmpeg_ir::math::interpolate(
            domain[0],
            domain[1],
            {
                let Some(value) = cadmpeg_core::convert::f64_from_index(index) else {
                    return Ok(None);
                };
                value
            } / {
                let Some(value) =
                    cadmpeg_core::convert::f64_from_index(COARSE_PCURVE_SEARCH_INTERVALS)
                else {
                    return Ok(None);
                };
                value
            },
        ) else {
            return Ok(None);
        };
        let parameter = parameter.get();
        let Some(candidate) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
                pcurve,
                parameter,
            ))?
        else {
            return Ok(None);
        };
        let distance = (candidate.u - point.u).hypot(candidate.v - point.v);
        if !distance.is_finite() {
            continue;
        }
        if closest.is_none_or(|(_, current)| distance < current) {
            closest = Some((parameter, distance));
        }
    }
    let Some((parameter, _)) = closest else {
        return Ok(None);
    };
    closest_pcurve_parameter_from_seed(ctx, pcurve, point, parameter)
}

fn closest_pcurve_parameter_from_seed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &PcurveGeometry,
    point: Point2,
    seed: f64,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    let Some((domain, periodic)) = pcurve_domain(pcurve) else {
        return Ok(None);
    };
    let mut parameter = if periodic {
        canonical_periodic_parameter(domain, true, seed)
    } else {
        seed.clamp(domain[0], domain[1])
    };
    // A fixed count of steps; each evaluation charges its own work.
    for _ in 0..LOCAL_PCURVE_SEARCH_STEPS {
        let Some(candidate) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
                pcurve,
                parameter,
            ))?
        else {
            return Ok(None);
        };
        let Some(tangent) =
            cadmpeg_ir::eval::finite_or_refusal(pcurve_tangent(ctx, pcurve, parameter))?
        else {
            return Ok(None);
        };
        let tangent_scale = tangent.u.abs().max(tangent.v.abs());
        if tangent_scale == 0.0 {
            return Ok(None);
        }
        let tangent = Point2::new(tangent.u / tangent_scale, tangent.v / tangent_scale);
        let speed_squared = tangent.u * tangent.u + tangent.v * tangent.v;
        let gradient = (candidate.u - point.u) * tangent.u + (candidate.v - point.v) * tangent.v;
        let Some(step) =
            cadmpeg_ir::math::product_quotient([gradient], [speed_squared, tangent_scale])
        else {
            return Ok(None);
        };
        let step = step.get();
        let next = if periodic {
            canonical_periodic_parameter(domain, true, parameter - step)
        } else {
            (parameter - step).clamp(domain[0], domain[1])
        };
        if next == parameter {
            return Ok(Some(next));
        }
        let next_fraction = cadmpeg_ir::math::parameter_fraction(next, domain[0], domain[1]);
        let current_fraction =
            cadmpeg_ir::math::parameter_fraction(parameter, domain[0], domain[1]);
        let (Some(next_fraction), Some(current_fraction)) = (next_fraction, current_fraction)
        else {
            return Ok(None);
        };
        if (next_fraction.get() - current_fraction.get()).abs() <= 64.0 * f64::EPSILON {
            return Ok(Some(next));
        }
        parameter = next;
    }
    Ok(Some(parameter))
}

#[cfg(test)]
pub(super) fn closest_pcurve_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &PcurveGeometry,
    point: Point2,
    seed: Option<f64>,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return Ok(None);
    };
    let Ok(degree) = usize::try_from(nurbs.degree()) else {
        return Ok(None);
    };
    let count = nurbs.pole_rows().count();
    let (Some(lower), Some(upper)) = (nurbs.knots().get(degree), nurbs.knots().get(count)) else {
        return Ok(None);
    };
    let domain = [*lower, *upper];
    if !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1] {
        return Ok(None);
    }
    if seed.is_some_and(|seed| !seed.is_finite()) {
        return Ok(None);
    }
    let search_seed = seed.map(|seed| canonical_periodic_parameter(domain, nurbs.periodic(), seed));
    let lanes = cadmpeg_ir::geometry::pcurve::evaluator::PcurveEvaluatorLanes::new(
        ctx,
        nurbs.pole_rows(),
        "IR pcurve control copy",
        "IR pcurve weight copy",
    )?;
    let Some(homogeneous) = homogeneous_pcurve_spans(
        ctx,
        degree,
        nurbs.knots(),
        lanes.points(),
        lanes.weights(),
        point,
    )?
    else {
        return Ok(None);
    };
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    let candidates = if degree != 1 || nurbs.weights().is_some() {
        let Some(candidates) =
            stationary_rational_distance_candidates(&homogeneous, search_seed, &geometry_budget)?
        else {
            return Ok(None);
        };
        closest_parameter_candidates(&candidates, search_seed, &geometry_budget)?
    } else {
        let candidates = nurbs
            .control_points()
            .windows(2)
            .enumerate()
            .filter_map(|(index, segment)| {
                let start = segment[0];
                let end = segment[1];
                let direction = Point2::new(end.u - start.u, end.v - start.v);
                let squared_length = direction.u * direction.u + direction.v * direction.v;
                if !squared_length.is_finite() || squared_length == 0.0 {
                    return None;
                }
                let fraction = (((point.u - start.u) * direction.u
                    + (point.v - start.v) * direction.v)
                    / squared_length)
                    .clamp(0.0, 1.0);
                let span_start = *nurbs.knots().get(index + 1)?;
                let span_end = *nurbs.knots().get(index + 2)?;
                if !span_start.is_finite() || !span_end.is_finite() || span_start >= span_end {
                    return None;
                }
                let projected = Point2::new(
                    start.u + fraction * direction.u,
                    start.v + fraction * direction.v,
                );
                let squared_distance =
                    (projected.u - point.u).powi(2) + (projected.v - point.v).powi(2);
                Some((
                    span_start + fraction * (span_end - span_start),
                    squared_distance,
                ))
            })
            .collect::<Vec<_>>();
        closest_parameter_candidates(&candidates, search_seed, &geometry_budget)?
    };
    candidates
        .map(|candidates| {
            lift_periodic_parameters(ctx, candidates.to_vec(), domain, nurbs.periodic(), seed)
        })
        .transpose()
}

struct HomogeneousCurveSpans<'ctx, const DIMENSION: usize> {
    extraction: ScopedRows<'ctx, HomogeneousBezierSpan<DIMENSION>>,
    coordinate_tolerance: f64,
}

#[cfg(test)]
fn homogeneous_pcurve_spans<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    degree: usize,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    point: Point2,
) -> Result<Option<HomogeneousCurveSpans<'ctx, 3>>, cadmpeg_core::CodecError> {
    let count = control_points.len();
    let Some(expected_knots) = count
        .checked_add(degree)
        .and_then(|length| length.checked_add(1))
    else {
        return Ok(None);
    };
    if degree == 0
        || count <= degree
        || knots.len() != expected_knots
        || knots.iter().any(|knot| !knot.is_finite())
        || !cadmpeg_ir::geometry::nurbs::knots_nondecreasing(knots, |count| {
            ctx.charge_work(count, "IR NURBS knot order")
        })?
        || control_points.iter().any(|control| !control.is_finite())
        || !point.is_finite()
    {
        return Ok(None);
    }
    if weights.is_some_and(|weights| {
        weights.len() != count
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
    }) {
        return Ok(None);
    }
    let coordinate_scale = control_points
        .iter()
        .flat_map(|control| [control.u, control.v])
        .chain([point.u, point.v])
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let mut controls = ctx.alloc_filled(count, [0.0; 3], "nx blend pcurve controls")?;
    for (index, control) in control_points.iter().enumerate() {
        let weight = weights.map_or(1.0, |weights| weights[index]);
        controls[index] = [
            weight * (control.u - point.u),
            weight * (control.v - point.v),
            weight,
        ];
    }
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return Ok(None);
    }
    let Some(spans) = homogeneous_spans(ctx, degree, knots, &controls)? else {
        return Ok(None);
    };
    Ok(Some(HomogeneousCurveSpans {
        extraction: spans,
        coordinate_tolerance: 64.0 * f64::EPSILON * coordinate_scale,
    }))
}

/// Temporary values whose storage stays reserved until they are dropped.
pub(super) struct ScopedValues<'ctx, T> {
    values: Vec<T>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx, T> ScopedValues<'ctx, T> {
    /// Empty values with storage reserved for `count` of them.
    pub(super) fn with_capacity(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        count: usize,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::decode::ResourceLimit> {
        let mut values = Vec::new();
        let storage = ctx.reserve_temporary_vec(&mut values, count, operation)?;
        Ok(Self { values, storage })
    }

    /// Append one value, reserving its storage first.
    fn push(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: T,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        ctx.reserve_scoped_vec_limit(&mut self.storage, &mut self.values, 1, operation)?;
        self.values.push(value);
        Ok(())
    }
}

impl<'ctx, T: Copy> ScopedValues<'ctx, T> {
    /// A temporary copy of `values`.
    pub(super) fn copy_of(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        values: &[T],
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::decode::ResourceLimit> {
        let (values, storage) = ctx.copy_temporary_slice(values, operation)?;
        Ok(Self { values, storage })
    }
}

impl<T> std::ops::Deref for ScopedValues<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<T> std::ops::DerefMut for ScopedValues<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

fn stationary_rational_distance_candidates<'ctx, const DIMENSION: usize>(
    homogeneous: &HomogeneousCurveSpans<'_, DIMENSION>,
    seed: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'ctx>,
) -> Result<Option<ScopedValues<'ctx, (f64, f64)>>, cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    let mut candidates = ScopedValues::with_capacity(ctx, 0, "nx stationary candidates")?;
    for span in ctx.admit_iter(&*homogeneous.extraction, "nx stationary span traversal")? {
        let Some(derivative) =
            rational_squared_distance_derivative(&span.controls, geometry_budget)?
        else {
            return Ok(None);
        };
        let Some(roots) = scalar_bezier_roots_with_budget(
            ScalarBezierSpan {
                domain: span.domain,
                controls: derivative,
            },
            geometry_budget,
        )?
        else {
            return geometry_budget.resource_refusal().map_or(Ok(None), Err);
        };
        // The span's ends, then its stationary parameters.
        let constant_seed = seed.filter(|seed| (span.domain[0]..=span.domain[1]).contains(seed));
        let roots: &[f64] = match &roots {
            ScalarBezierRoots::Constant => constant_seed.as_slice(),
            ScalarBezierRoots::Isolated(roots) => roots,
        };
        for &parameter in [span.domain[0], span.domain[1]]
            .iter()
            .chain(ctx.admit_iter(roots, "nx stationary parameters")?)
        {
            let distance = homogeneous_residual_distance(
                &span.controls,
                parameter,
                span.domain,
                geometry_budget,
            )?;
            candidates.push(
                ctx,
                (
                    parameter,
                    if distance <= homogeneous.coordinate_tolerance {
                        0.0
                    } else {
                        distance * distance
                    },
                ),
                "nx stationary candidates",
            )?;
        }
    }
    Ok(Some(candidates))
}

fn rational_squared_distance_derivative<'ctx, const DIMENSION: usize>(
    controls: &[[f64; DIMENSION]],
    geometry_budget: &GeometryWorkBudget<'ctx>,
) -> Result<Option<ScopedValues<'ctx, f64>>, cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    // For residual R/W, half the squared-distance derivative has numerator
    // ((R·R')W - (R·R)W'). Positive weights make its roots exactly the finite
    // stationary parameters of the rational span.
    // A common homogeneous factor cannot change stationary parameters.
    let mut scale = 0.0_f64;
    for control in ctx.admit_iter(controls, "nx rational derivative scale")? {
        for value in control {
            if !value.is_finite() {
                return Ok(None);
            }
            scale = scale.max(value.abs());
        }
    }
    let Some(exponent) = cadmpeg_ir::math::power_of_two_bound(scale) else {
        return Ok(None);
    };
    let mut normalized_controls = ScopedValues::with_capacity(
        ctx,
        controls.len(),
        "nx rational derivative normalized controls",
    )?;
    let mut weight =
        ScopedValues::with_capacity(ctx, controls.len(), "nx rational derivative weights")?;
    for control in ctx.admit_iter(controls, "nx rational derivative normalization")? {
        let mut normalized = *control;
        for value in &mut normalized {
            let Some(scaled) = cadmpeg_ir::math::scale_power_of_two(*value, -exponent) else {
                return Ok(None);
            };
            *value = scaled.get();
        }
        normalized_controls.values.push(normalized);
        weight.values.push(normalized[DIMENSION - 1]);
    }
    let weight_derivative = difference_controls(ctx, &weight)?;
    let mut residual_squared: Option<ScopedValues<'ctx, f64>> = None;
    let mut residual_derivative: Option<ScopedValues<'ctx, f64>> = None;
    for axis in 0..DIMENSION - 1 {
        let mut residual =
            ScopedValues::with_capacity(ctx, controls.len(), "nx rational derivative residuals")?;
        for control in ctx.admit_iter(&*normalized_controls, "nx rational derivative residuals")? {
            residual.values.push(control[axis]);
        }
        let derivative = difference_controls(ctx, &residual)?;
        let Some(squared) = bernstein_product(ctx, &residual, &residual)? else {
            return Ok(None);
        };
        let Some(differentiated) = bernstein_product(ctx, &residual, &derivative)? else {
            return Ok(None);
        };
        residual_squared = match residual_squared {
            Some(accumulated) => add_bernstein_polynomials(ctx, accumulated, &squared)?,
            None => Some(squared),
        };
        residual_derivative = match residual_derivative {
            Some(accumulated) => add_bernstein_polynomials(ctx, accumulated, &differentiated)?,
            None => Some(differentiated),
        };
        if residual_squared.is_none() || residual_derivative.is_none() {
            return Ok(None);
        }
    }
    let (Some(residual_squared), Some(residual_derivative)) =
        (residual_squared, residual_derivative)
    else {
        return Ok(None);
    };
    let Some(first) = bernstein_product(ctx, &residual_derivative, &weight)? else {
        return Ok(None);
    };
    let Some(second) = bernstein_product(ctx, &residual_squared, &weight_derivative)? else {
        return Ok(None);
    };
    subtract_bernstein_polynomials(ctx, first, &second)
}

fn difference_controls<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    values: &[f64],
) -> Result<ScopedValues<'ctx, f64>, cadmpeg_core::decode::ResourceLimit> {
    let count = if values.is_empty() {
        0
    } else {
        values.len() - 1
    };
    let mut differences =
        ScopedValues::with_capacity(ctx, count, "nx rational derivative differences")?;
    for next in ctx.admit_iter(&(1..values.len()), "nx rational derivative differences")? {
        differences.values.push(values[next] - values[next - 1]);
    }
    Ok(differences)
}

/// The binomial coefficients of `degree`, one per index.
fn binomial_row<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    degree: usize,
) -> Result<Option<ScopedValues<'ctx, f64>>, cadmpeg_core::decode::ResourceLimit> {
    let Some(count) = degree.checked_add(1) else {
        return Ok(None);
    };
    let mut row = ScopedValues::with_capacity(ctx, count, "nx binomial coefficients")?;
    for index in ctx.admit_iter(&(0..=degree), "nx binomial coefficients")? {
        // The coefficient multiplies the smaller of its two factor counts.
        ctx.charge_work_limit(
            cadmpeg_core::decode::u64_from_index(index.min(degree - index)),
            "nx binomial coefficient factors",
        )?;
        let Some(coefficient) = binomial_coefficient(degree, index) else {
            return Ok(None);
        };
        row.values.push(coefficient);
    }
    Ok(Some(row))
}

fn bernstein_product<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    first: &[f64],
    second: &[f64],
) -> Result<Option<ScopedValues<'ctx, f64>>, cadmpeg_core::decode::ResourceLimit> {
    let (Some(first_degree), Some(second_degree)) =
        (first.len().checked_sub(1), second.len().checked_sub(1))
    else {
        return Ok(None);
    };
    let Some(degree) = first_degree.checked_add(second_degree) else {
        return Ok(None);
    };
    let Some(count) = degree.checked_add(1) else {
        return Ok(None);
    };
    // Every coefficient of the three rows enters some product term, so each
    // row is formed once.
    let (Some(first_row), Some(second_row), Some(product_row)) = (
        binomial_row(ctx, first_degree)?,
        binomial_row(ctx, second_degree)?,
        binomial_row(ctx, degree)?,
    ) else {
        return Ok(None);
    };
    let mut product = ScopedValues::with_capacity(ctx, count, "nx Bernstein product")?;
    for index in ctx.admit_iter(&(0..=degree), "nx Bernstein product")? {
        let denominator = product_row[index];
        let lower = index - index.min(second_degree);
        let upper = index.min(first_degree);
        let Some(value) = ctx
            .admit_iter(&(lower..=upper), "nx Bernstein product terms")?
            .map(|first_index| {
                let second_index = index - first_index;
                Some(
                    first[first_index]
                        * second[second_index]
                        * first_row[first_index]
                        * second_row[second_index]
                        / denominator,
                )
            })
            .sum::<Option<f64>>()
            .filter(|value| value.is_finite())
        else {
            return Ok(None);
        };
        product.values.push(value);
    }
    Ok(Some(product))
}

#[cfg(test)]
mod binomial_numeric_tests {
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn binomial_coefficient_refuses_inexact_factor() {
        assert_eq!(super::binomial_coefficient(9_007_199_254_740_993, 1), None);
    }
}

fn binomial_coefficient(n: usize, k: usize) -> Option<f64> {
    let k = k.min(n.checked_sub(k)?);
    (1..=k).try_fold(1.0, |value, index| {
        let next = value * cadmpeg_core::convert::f64_from_index(n - k + index)?
            / cadmpeg_core::convert::f64_from_index(index)?;
        next.is_finite().then_some(next)
    })
}

fn add_bernstein_polynomials<'ctx>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut first: ScopedValues<'ctx, f64>,
    second: &[f64],
) -> Result<Option<ScopedValues<'ctx, f64>>, cadmpeg_core::decode::ResourceLimit> {
    if first.len() != second.len() {
        return Ok(None);
    }
    for index in ctx.admit_iter(&(0..second.len()), "nx Bernstein sum")? {
        first.values[index] += second[index];
        if !first.values[index].is_finite() {
            return Ok(None);
        }
    }
    Ok(Some(first))
}

fn subtract_bernstein_polynomials<'ctx>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut first: ScopedValues<'ctx, f64>,
    second: &[f64],
) -> Result<Option<ScopedValues<'ctx, f64>>, cadmpeg_core::decode::ResourceLimit> {
    if first.len() != second.len() {
        return Ok(None);
    }
    for index in ctx.admit_iter(&(0..second.len()), "nx Bernstein difference")? {
        first.values[index] -= second[index];
        if !first.values[index].is_finite() {
            return Ok(None);
        }
    }
    Ok(Some(first))
}

pub(in crate::decode) enum ScalarBezierRoots<'ctx> {
    Constant,
    Isolated(ScopedValues<'ctx, f64>),
}

pub(super) struct ScalarBezierSpan<'ctx> {
    pub(super) domain: [f64; 2],
    pub(super) controls: ScopedValues<'ctx, f64>,
}

pub(super) fn scalar_bezier_roots_with_budget<'ctx>(
    span: ScalarBezierSpan<'ctx>,
    geometry_budget: &GeometryWorkBudget<'ctx>,
) -> Result<Option<ScalarBezierRoots<'ctx>>, cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    let mut scale = 1.0_f64;
    let mut constant = true;
    for value in ctx.admit_iter(&*span.controls, "nx Bezier root scale")? {
        scale = scale.max(value.abs());
        constant &= *value == 0.0;
    }
    let tolerance = 64.0 * f64::EPSILON * scale;
    if constant {
        return Ok(Some(ScalarBezierRoots::Constant));
    }
    let mut parameters = ScopedValues::with_capacity(ctx, 0, "nx Bezier root parameters")?;
    if span
        .controls
        .first()
        .is_some_and(|value| value.abs() <= tolerance)
    {
        parameters.push(ctx, span.domain[0], "nx Bezier root parameters")?;
    }
    if span
        .controls
        .last()
        .is_some_and(|value| value.abs() <= tolerance)
    {
        parameters.push(ctx, span.domain[1], "nx Bezier root parameters")?;
    }
    let domain = span.domain;
    let mut intervals = ScopedValues::with_capacity(ctx, 1, "nx Bezier root intervals")?;
    intervals.values.push(span);
    // Each probed interval is one unit of the adaptive geometry budget, which
    // draws on the session work allowance.
    while let Some(span) = intervals.values.pop() {
        if !geometry_budget.charge() {
            return geometry_budget.resource_refusal().map_or(Ok(None), Err);
        }
        if scalar_bernstein_sign_variations(ctx, &span.controls)? == 0 {
            continue;
        }
        let Some(middle) = cadmpeg_ir::math::interpolate(span.domain[0], span.domain[1], 0.5)
        else {
            return Ok(None);
        };
        let middle = middle.get();
        if middle == span.domain[0] || middle == span.domain[1] {
            let first_value =
                scalar_bezier_value(&span.controls, span.domain[0], span.domain, geometry_budget)?
                    .abs();
            let second_value =
                scalar_bezier_value(&span.controls, span.domain[1], span.domain, geometry_budget)?
                    .abs();
            let (parameter, value) = if first_value.total_cmp(&second_value).is_le() {
                (span.domain[0], first_value)
            } else {
                (span.domain[1], second_value)
            };
            if value <= tolerance {
                parameters.push(ctx, parameter, "nx Bezier root parameters")?;
            }
            continue;
        }
        let (first, second) = subdivide_scalar_bezier_span(span, middle, geometry_budget)?;
        if first.controls.last().is_some_and(|value| *value == 0.0) {
            parameters.push(ctx, middle, "nx Bezier root parameters")?;
        }
        intervals.push(ctx, second, "nx Bezier root intervals")?;
        intervals.push(ctx, first, "nx Bezier root intervals")?;
    }
    if ctx
        .stable_sort_by(
            &mut parameters,
            |value| value,
            f64::total_cmp,
            "nx Bezier root parameters sort",
        )
        .is_err()
    {
        return geometry_budget.resource_refusal().map_or(Ok(None), Err);
    }
    ctx.charge_work_limit(
        cadmpeg_core::decode::u64_from_index(parameters.len()),
        "nx Bezier root parameters dedup",
    )?;
    parameters.values.dedup_by(|first, second| {
        let first = cadmpeg_ir::math::parameter_fraction(*first, domain[0], domain[1]);
        let second = cadmpeg_ir::math::parameter_fraction(*second, domain[0], domain[1]);
        first.zip(second).is_some_and(|(first, second)| {
            (first.get() - second.get()).abs() <= 64.0 * f64::EPSILON
        })
    });
    Ok(Some(ScalarBezierRoots::Isolated(parameters)))
}

fn scalar_bernstein_sign_variations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    controls: &[f64],
) -> Result<usize, cadmpeg_core::decode::ResourceLimit> {
    // Bernstein-form Descartes variation bounds the roots in the open span.
    // Exact zero controls do not contribute a sign.
    Ok(ctx
        .admit_iter(controls, "nx Bernstein sign variations")?
        .copied()
        .filter(|value| *value != 0.0)
        .map(f64::is_sign_positive)
        .fold((None, 0), |(previous, variations), positive| {
            (
                Some(positive),
                variations + usize::from(previous.is_some_and(|previous| previous != positive)),
            )
        })
        .1)
}

/// Charge the de Casteljau work of `count` controls: one level of
/// `count - 1` combinations, then one fewer at each further level, for
/// `count * (count - 1) / 2` in all. An overflowing pair count refuses
/// through the context before any work is charged.
fn charge_de_casteljau_work(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
    let pairs = if count == 0 {
        Some(0)
    } else if count.is_multiple_of(2) {
        count.checked_sub(1).and_then(|previous| (count / 2).checked_mul(previous))
    } else {
        count.checked_mul(count / 2)
    };
    let Some(pairs) = pairs else {
        drop(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
        return ctx.resource_refusal().map_or(Ok(()), Err);
    };
    ctx.charge_work_limit(cadmpeg_core::decode::u64_from_index(pairs), operation)
}

fn subdivide_scalar_bezier_span<'ctx>(
    span: ScalarBezierSpan<'ctx>,
    middle: f64,
    geometry_budget: &GeometryWorkBudget<'ctx>,
) -> Result<(ScalarBezierSpan<'ctx>, ScalarBezierSpan<'ctx>), cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    let count = span.controls.len();
    let mut levels = span.controls;
    let mut first = ScopedValues::with_capacity(ctx, count, "nx first Bezier subdivision")?;
    let mut second = ScopedValues::with_capacity(ctx, count, "nx second Bezier subdivision")?;
    charge_de_casteljau_work(ctx, count, "nx Bezier subdivision")?;
    for level in ctx.admit_iter(&(0..count), "nx Bezier subdivision levels")? {
        first.values.push(levels[0]);
        second.values.push(levels[count - level - 1]);
        for index in 0..count - level - 1 {
            levels[index] = levels[index].midpoint(levels[index + 1]);
        }
    }
    ctx.charge_work_limit(
        cadmpeg_core::decode::u64_from_index(second.len()),
        "nx second Bezier subdivision reversal",
    )?;
    second.values.reverse();
    Ok((
        ScalarBezierSpan {
            domain: [span.domain[0], middle],
            controls: first,
        },
        ScalarBezierSpan {
            domain: [middle, span.domain[1]],
            controls: second,
        },
    ))
}

fn scalar_bezier_value(
    controls: &[f64],
    parameter: f64,
    domain: [f64; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<f64, cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    let fraction = cadmpeg_ir::math::parameter_fraction(parameter, domain[0], domain[1])
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get);
    let mut values = ScopedValues::copy_of(ctx, controls, "nx scalar Bezier evaluation")?;
    charge_de_casteljau_work(ctx, values.len(), "nx scalar Bezier evaluation")?;
    for level in 1..values.len() {
        for index in 0..values.len() - level {
            values[index] = (1.0 - fraction) * values[index] + fraction * values[index + 1];
        }
    }
    Ok(values[0])
}

pub(super) fn homogeneous_residual_distance<const DIMENSION: usize>(
    controls: &[[f64; DIMENSION]],
    parameter: f64,
    domain: [f64; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<f64, cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    let fraction = cadmpeg_ir::math::parameter_fraction(parameter, domain[0], domain[1])
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get);
    let mut values = ScopedValues::copy_of(ctx, controls, "nx rational Bezier evaluation")?;
    charge_de_casteljau_work(ctx, values.len(), "nx rational Bezier evaluation")?;
    for level in 1..values.len() {
        for index in 0..values.len() - level {
            values[index] = std::array::from_fn(|axis| {
                (1.0 - fraction) * values[index][axis] + fraction * values[index + 1][axis]
            });
        }
    }
    Ok(values[0][..DIMENSION - 1]
        .iter()
        .map(|value| value / values[0][DIMENSION - 1])
        .fold(0.0, f64::hypot))
}

/// The parameters of the least-distance candidates, nearest `seed` first.
fn closest_parameter_candidates<'ctx>(
    candidates: &[(f64, f64)],
    seed: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'ctx>,
) -> Result<Option<ScopedValues<'ctx, f64>>, cadmpeg_core::decode::ResourceLimit> {
    let ctx = geometry_budget.charges;
    let Some(minimum_distance) = ctx
        .admit_iter(candidates, "nx closest parameter minimum")?
        .map(|candidate| candidate.1)
        .min_by(f64::total_cmp)
    else {
        return Ok(None);
    };
    let mut nearest = ScopedValues::with_capacity(ctx, 0, "nx closest parameter minima")?;
    for candidate in ctx.admit_iter(candidates, "nx closest parameter minima")? {
        let scale = candidate
            .1
            .abs()
            .max(minimum_distance.abs())
            .max(f64::MIN_POSITIVE);
        if (candidate.1 - minimum_distance).abs() <= 128.0 * f64::EPSILON * scale {
            nearest.push(ctx, candidate.0, "nx closest parameter minima")?;
        }
    }
    if ctx
        .stable_sort_by(
            &mut nearest,
            |value| value,
            |first, second| {
                seed.map_or_else(
                    || first.total_cmp(second),
                    |seed| {
                        (first - seed)
                            .abs()
                            .total_cmp(&(second - seed).abs())
                            .then_with(|| first.total_cmp(second))
                    },
                )
            },
            "nx closest parameter minima sort",
        )
        .is_err()
    {
        return geometry_budget.resource_refusal().map_or(Ok(None), Err);
    }
    ctx.charge_work_limit(
        cadmpeg_core::decode::u64_from_index(nearest.len()),
        "nx closest parameter minima dedup",
    )?;
    nearest
        .values
        .dedup_by(|first, second| first.to_bits() == second.to_bits());
    Ok((!nearest.is_empty()).then_some(nearest))
}

fn canonical_periodic_parameter(domain: [f64; 2], periodic: bool, parameter: f64) -> f64 {
    if !periodic {
        return parameter;
    }
    cadmpeg_ir::math::wrap_parameter(parameter, domain[0], domain[1])
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get)
}

/// Lift `parameter` to the period of `domain` nearest `seed`.
fn lift_periodic_parameter_in_domain(parameter: f64, domain: [f64; 2], seed: f64) -> f64 {
    let period = domain[1] - domain[0];
    if period.is_finite() {
        return super::offset::lift_periodic_parameter(parameter, seed, period);
    }
    let half_period = domain[1] * 0.5 - domain[0] * 0.5;
    let shifted = [
        parameter,
        (parameter + half_period) + half_period,
        (parameter - half_period) - half_period,
    ];
    shifted
        .into_iter()
        .filter(|candidate| candidate.is_finite())
        .min_by(|left, right| {
            (left * 0.5 - seed * 0.5)
                .abs()
                .total_cmp(&(right * 0.5 - seed * 0.5).abs())
        })
        .unwrap_or(parameter)
}

/// The order lifted parameters take: nearest `seed` first, then ascending.
fn lifted_parameter_order(
    domain: [f64; 2],
    seed: f64,
) -> impl Fn(&f64, &f64) -> std::cmp::Ordering {
    let period = domain[1] - domain[0];
    move |first, second| {
        if period.is_finite() {
            (first - seed)
                .abs()
                .total_cmp(&(second - seed).abs())
                .then_with(|| first.total_cmp(second))
        } else {
            (first * 0.5 - seed * 0.5)
                .abs()
                .total_cmp(&(second * 0.5 - seed * 0.5).abs())
                .then_with(|| first.total_cmp(second))
        }
    }
}

#[cfg(test)]
fn lift_periodic_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut parameters: Vec<f64>,
    domain: [f64; 2],
    periodic: bool,
    seed: Option<f64>,
) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
    let Some(seed) = seed.filter(|_| periodic) else {
        return Ok(parameters);
    };
    for parameter in &mut parameters {
        *parameter = lift_periodic_parameter_in_domain(*parameter, domain, seed);
    }
    ctx.stable_sort_by(
        &mut parameters,
        |value| value,
        lifted_parameter_order(domain, seed),
        "nx lifted periodic parameters sort",
    )?;
    parameters.dedup_by(|first, second| first.to_bits() == second.to_bits());
    Ok(parameters)
}

/// The first of `parameters` once each is lifted to the period nearest
/// `seed` and they are ordered nearest the seed first; the first parameter
/// when no periodic seed applies.
fn nearest_lifted_periodic_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &[f64],
    domain: [f64; 2],
    periodic: bool,
    seed: Option<f64>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Some(seed) = seed.filter(|_| periodic) else {
        return Ok(parameters.first().copied());
    };
    let order = lifted_parameter_order(domain, seed);
    Ok(ctx
        .admit_iter(parameters, "nx lifted periodic parameters")?
        .map(|parameter| lift_periodic_parameter_in_domain(*parameter, domain, seed))
        .min_by(|first, second| order(first, second)))
}

fn spine_contact_point_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    radius: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    let mut contact_seeds = BlendContactSeedCache::default();
    spine_contact_point_with_index_and_budget_and_options(
        index,
        &SpineContactLocation {
            support,
            spine,
            parameter,
            radius,
        },
        depth,
        false,
        &mut contact_seeds,
        geometry_budget,
    )
}

// Keep the support relation, recursion policy, bounded seed cache, and work
// slice together so nested contact evaluation cannot hide an allocation.
fn spine_contact_point_with_index_and_budget_and_options<'k>(
    index: &'k cadmpeg_ir::index::ModelIndex<'_>,
    spine_contact_location: &SpineContactLocation<'k>,
    depth: usize,
    allow_offset_contact: bool,
    contact_seeds: &mut BlendContactSeedCache<'k>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    let &SpineContactLocation {
        support,
        spine,
        parameter,
        radius,
    } = spine_contact_location;

    (|| -> Option<Result<Point3, cadmpeg_core::decode::ResourceLimit>> {
        (depth < 32).then_some(())?;
        if let Some(pcurve) = match spine_contact_pcurve_with_index(
            index,
            support,
            spine,
            radius,
            depth + 1,
            geometry_budget.charges,
        ) {
            Ok(value) => value,
            Err(limit) => return Some(Err(limit)),
        } {
            let uv = match cadmpeg_ir::eval::decode::pcurve_uv(
                geometry_budget.charges,
                pcurve,
                parameter,
            ) {
                Ok(uv) => uv,
                Err(EvaluationFailure::ResourceLimit(limit)) => return Some(Err(limit)),
                Err(_) => return None,
            };
            return decoded_surface_point_inner_with_budget(
                index,
                support,
                uv.u,
                uv.v,
                depth + 1,
                geometry_budget,
            )
            .transpose();
        }
        if !allow_offset_contact {
            return None;
        }
        let lineage = match surface_offset_lineage_with_index(
            index,
            support,
            depth + 1,
            geometry_budget.charges,
        ) {
            Ok(value) => value,
            Err(limit) => return Some(Err(limit)),
        };
        let support_carrier = if let Some((base, _)) = lineage {
            match index.surfaces(base.as_str(), geometry_budget.charges) {
                Ok(value) => value,
                Err(limit) => return Some(Err(limit)),
            }
        } else {
            None
        };
        let support_has_nurbs_parameterization = support_carrier.is_some_and(|surface| {
            matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            )
        });
        if !support_has_nurbs_parameterization {
            return None;
        }
        spine_contact_point_from_offset_side_with_index_and_budget(
            index,
            &SpineContactLocation {
                support,
                spine,
                parameter,
                radius,
            },
            depth + 1,
            contact_seeds,
            geometry_budget,
        )
        .transpose()
    })()
    .transpose()
}

fn spine_contact_point_from_offset_side_with_index_and_budget<'k>(
    index: &'k cadmpeg_ir::index::ModelIndex<'_>,
    spine_contact_location: &SpineContactLocation<'k>,
    depth: usize,
    contact_seeds: &mut BlendContactSeedCache<'k>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    let &SpineContactLocation {
        support,
        spine,
        parameter,
        radius,
    } = spine_contact_location;

    (|| -> Option<Result<Point3, cadmpeg_core::decode::ResourceLimit>> {
        (depth < 32).then_some(())?;
        let tolerance = index.ir().tolerances.linear.get();
        if !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let center =
            match model_curve_point_with_index_and_budget(index, spine, parameter, geometry_budget)
            {
                Ok(Some(center)) => center,
                Ok(None) => return None,
                Err(limit) => return Some(Err(limit)),
            };
        let tangent = match model_curve_tangent_with_index_and_budget(
            index,
            spine,
            parameter,
            geometry_budget,
        ) {
            Ok(Some(tangent)) => tangent,
            Ok(None) => return None,
            Err(limit) => return Some(Err(limit)),
        };
        let procedurals =
            match index.procedural_curves_for_curve(spine.as_str(), geometry_budget.charges) {
                Ok(value) => value?,
                Err(limit) => return Some(Err(limit)),
            };
        let mut selected = None;
        for candidate in procedurals {
            if let Err(limit) = geometry_budget
                .charges
                .charge_work_limit(1, "NX spine offset contact construction scan")
            {
                return Some(Err(limit));
            }
            if let ProceduralCurveDefinition::Intersection { context, .. } = candidate.definition()
            {
                selected = Some((*candidate, context));
                break;
            }
        }
        let (procedural, context) = selected?;
        let contact_fit_tolerance = procedural
            .cache_fit_tolerance()
            .filter(|fit| fit.get() > 0.0)
            .map_or(tolerance, |fit| tolerance.max(fit.get()));
        let mut offset_surfaces = [None, None];
        for (slot, side) in offset_surfaces.iter_mut().zip(context.sides()) {
            let Some(surface) = side.surface.as_ref() else {
                continue;
            };
            // A contact chart can belong to a neighboring offset of the same support.
            let distance = match constant_surface_offset_between_with_index(
                index,
                support,
                surface,
                depth + 1,
                geometry_budget.charges,
            ) {
                Ok(Some(distance)) => distance,
                Ok(None) => continue,
                Err(limit) => return Some(Err(limit)),
            };
            if blend_contact_offset_matches(0.0, distance, radius) {
                *slot = Some((surface, distance.abs()));
            }
        }
        // Two sides by at most two offset carriers state at most four
        // candidates; exactly one is admitted.
        let mut candidate = None;
        let mut candidate_count = 0_usize;
        for side in context.sides() {
            let (Some(side_surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let side_uv = match cadmpeg_ir::eval::decode::pcurve_uv(
                geometry_budget.charges,
                &pcurve.geometry,
                parameter,
            ) {
                Ok(side_uv) => side_uv,
                Err(EvaluationFailure::ResourceLimit(limit)) => return Some(Err(limit)),
                Err(_) => continue,
            };
            let side_point = match decoded_surface_point_inner_with_budget(
                index,
                side_surface,
                side_uv.u,
                side_uv.v,
                depth + 1,
                geometry_budget,
            ) {
                Ok(Some(point)) => point,
                Ok(None) => continue,
                Err(limit) => return Some(Err(limit)),
            };
            for (offset_surface, offset_distance) in offset_surfaces.iter().flatten() {
                let cached_seed = match contact_seeds.seed_for(
                    geometry_budget.charges,
                    support,
                    spine,
                    parameter,
                    offset_surface,
                ) {
                    Ok(seed) => seed,
                    Err(limit) => return Some(Err(limit)),
                };
                let inverse_seed = if cached_seed.is_some() {
                    cached_seed
                } else if let Some(domain) = match surface_parameter_domain_with_index(
                    index,
                    offset_surface,
                    geometry_budget.charges,
                ) {
                    Ok(value) => value,
                    Err(limit) => return Some(Err(limit)),
                } {
                    match coarse_model_surface_parameters(
                        index,
                        offset_surface,
                        side_point,
                        domain,
                        geometry_budget,
                    ) {
                        Ok(seed) => seed,
                        Err(limit) => return Some(Err(limit)),
                    }
                } else {
                    None
                };
                let Some(inverse_seed) = inverse_seed else {
                    continue;
                };
                let parameters = match refine_offset_surface_parameters_with_index_and_budget(
                    index,
                    offset_surface,
                    side_point,
                    inverse_seed,
                    contact_fit_tolerance,
                    geometry_budget,
                ) {
                    Ok(parameters) => parameters,
                    Err(limit) => return Some(Err(limit)),
                };
                let Some(parameters) = parameters else {
                    continue;
                };
                let offset_point = match decoded_surface_point_inner_with_budget(
                    index,
                    offset_surface,
                    parameters.u,
                    parameters.v,
                    depth + 1,
                    geometry_budget,
                ) {
                    Ok(Some(point)) => point,
                    Ok(None) => continue,
                    Err(limit) => return Some(Err(limit)),
                };
                let offset_fit = Point3::distance(offset_point, side_point);
                if offset_fit > contact_fit_tolerance {
                    continue;
                }
                let reproduced = match decoded_surface_point_inner_with_budget(
                    index,
                    support,
                    parameters.u,
                    parameters.v,
                    depth + 1,
                    geometry_budget,
                ) {
                    Ok(Some(point)) => point,
                    Ok(None) => continue,
                    Err(limit) => return Some(Err(limit)),
                };
                let offset_error =
                    (Point3::distance(side_point, reproduced) - offset_distance).abs();
                if offset_error > contact_fit_tolerance {
                    continue;
                }
                let Some(radial) = FiniteVector3::new(Vector3::new(
                    reproduced.x - center.x,
                    reproduced.y - center.y,
                    reproduced.z - center.z,
                )) else {
                    continue;
                };
                let radial_length = radial.norm();
                if !radial_length.is_finite()
                    || (radial_length - radius).abs() > contact_fit_tolerance
                {
                    continue;
                }
                let Some(radial) = radial.unit_nonzero() else {
                    continue;
                };
                // The gate is the stated fit tolerance over the stated radius. A
                // floor under it would admit a contact the source's own tolerance
                // rejects, so a large radius states a strict gate and keeps it.
                let angular_tolerance = contact_fit_tolerance / radius;
                if radial.dot(tangent).abs() > angular_tolerance {
                    continue;
                }
                if candidate_count == 0 {
                    candidate = Some((reproduced, (*offset_surface, parameters)));
                }
                candidate_count += 1;
            }
        }
        let (candidate, (offset_surface, parameters)) =
            candidate.filter(|_| candidate_count == 1)?;
        if let Err(limit) = contact_seeds.remember(
            (support, spine, offset_surface),
            parameter,
            parameters,
            geometry_budget,
        ) {
            return Some(Err(limit));
        }
        Some(Ok(candidate))
    })()
    .transpose()
}

pub(super) fn spine_contact_pcurve_with_index<'a>(
    index: &cadmpeg_ir::index::ModelIndex<'a>,
    support: &SurfaceId,
    spine: &CurveId,
    radius: f64,
    depth: usize,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<&'a PcurveGeometry>, cadmpeg_core::decode::ResourceLimit> {
    let _depth = ctx.enter_nested_limit("NX spine contact lookup depth")?;
    if depth >= 32 {
        return Ok(None);
    }
    let Some(procedural) = index.procedural_curves_for_curve(spine.as_str(), ctx)? else {
        return Ok(None);
    };
    let mut context = None;
    for candidate in procedural {
        ctx.charge_work_limit(1, "NX spine contact construction scan")?;
        if let ProceduralCurveDefinition::Intersection {
            context: candidate, ..
        } = candidate.definition()
        {
            context = Some(candidate);
            break;
        }
    }
    let Some(context) = context else {
        return Ok(None);
    };
    let mut selected = None;
    for side in context.sides() {
        let (Some(side_surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
            continue;
        };
        let Some(offset) = constant_surface_offset_between_with_index(
            index,
            support,
            side_surface,
            depth + 1,
            ctx,
        )?
        else {
            continue;
        };
        if blend_contact_offset_matches(0.0, offset, radius)
            && selected.replace(&pcurve.geometry).is_some()
        {
            return Ok(None);
        }
    }
    Ok(selected)
}

#[cfg(test)]
pub(super) fn constant_surface_offset_between(
    ir: &CadIr,
    support: &SurfaceId,
    offset_surface: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, cadmpeg_ir::index::StandardIndex);
    constant_surface_offset_between_with_index(
        &index,
        support,
        offset_surface,
        depth,
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
}

fn constant_surface_offset_between_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    offset_surface: &SurfaceId,
    depth: usize,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let _depth = ctx.enter_nested_limit("NX constant surface offset depth")?;
    let Some((support_base, support_offset)) =
        surface_offset_lineage_with_index(index, support, depth + 1, ctx)?
    else {
        return Ok(None);
    };
    let Some((offset_base, offset_distance)) =
        surface_offset_lineage_with_index(index, offset_surface, depth + 1, ctx)?
    else {
        return Ok(None);
    };
    if same_text(
        ctx,
        support_base.as_str(),
        offset_base.as_str(),
        "NX constant surface offset base comparison",
    )? {
        return Ok(Some(offset_distance - support_offset));
    }
    let Some(support_carrier) = index.surfaces(support_base.as_str(), ctx)? else {
        return Ok(None);
    };
    let Some(offset_carrier) = index.surfaces(offset_base.as_str(), ctx)? else {
        return Ok(None);
    };
    let base_offset =
        match analytic_surface_offset(&support_carrier.geometry, &offset_carrier.geometry) {
            Some(offset) => Some(offset),
            None => {
                blend_surface_offset_with_index(index, support_base, offset_base, depth + 1, ctx)?
            }
        };
    Ok(base_offset.map(|base_offset| base_offset + offset_distance - support_offset))
}

fn blend_surface_offset_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    offset: &SurfaceId,
    depth: usize,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let _depth = ctx.enter_nested_limit("NX blend surface offset depth")?;
    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition {
        supports: support_carriers,
        spine: support_spine,
        radius: support_radius,
        reversed: support_reversed,
    }) = blend_surface_definition_with_index(index, support, ctx)?
    else {
        return Ok(None);
    };
    let Some(CircularBlendDefinition {
        supports: offset_carriers,
        spine: offset_spine,
        radius: offset_radius,
        reversed: offset_reversed,
    }) = blend_surface_definition_with_index(index, offset, ctx)?
    else {
        return Ok(None);
    };
    if !same_text(
        ctx,
        support_spine.as_str(),
        offset_spine.as_str(),
        "NX blend surface offset spine comparison",
    )? {
        return Ok(None);
    }
    let distance = offset_radius - support_radius;
    let magnitude = distance.abs();
    let mut matches = 0;
    for permutation in [[0usize, 1usize], [1usize, 0usize]] {
        let mut matched = true;
        for (support_index, offset_index) in permutation.into_iter().enumerate() {
            if support_reversed[support_index] != offset_reversed[offset_index] {
                matched = false;
                break;
            }
            let distance = constant_surface_offset_between_with_index(
                index,
                support_carriers[support_index],
                offset_carriers[offset_index],
                depth + 1,
                ctx,
            )?;
            if !distance
                .is_some_and(|distance| blend_contact_offset_matches(0.0, distance, magnitude))
            {
                matched = false;
                break;
            }
        }
        matches += usize::from(matched);
    }
    Ok((matches == 1).then_some(distance))
}

pub(super) fn analytic_surface_offset(
    support: &SurfaceGeometry,
    offset: &SurfaceGeometry,
) -> Option<f64> {
    match (support, offset) {
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
        ) if {
            let support_normal = plane_surface.frame().axis().as_raw();
            let support_u = plane_surface.frame().reference().as_raw();
            let offset_normal = plane_surface_2.frame().axis().as_raw();
            let offset_u = plane_surface_2.frame().reference().as_raw();
            support_normal == offset_normal && support_u == offset_u
        } =>
        {
            let support_origin = plane_surface.origin();
            let support_normal = plane_surface.frame().axis().as_raw();
            let offset_origin = plane_surface_2.origin();
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let distance = delta.dot(*support_normal);
            let residual = Vector3::new(
                delta.x - distance * support_normal.x,
                delta.y - distance * support_normal.y,
                delta.z - distance * support_normal.z,
            );
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                distance,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (residual.dot(residual) <= tolerance * tolerance).then_some(distance)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
        ) if {
            let support_origin = cylinder_surface.origin();
            let support_axis = cylinder_surface.frame().axis().as_raw();
            let support_ref = cylinder_surface.frame().reference().as_raw();
            let offset_origin = cylinder_surface_2.origin();
            let offset_axis = cylinder_surface_2.frame().axis().as_raw();
            let offset_ref = cylinder_surface_2.frame().reference().as_raw();
            support_origin == offset_origin
                && support_axis == offset_axis
                && support_ref == offset_ref
        } =>
        {
            let support_radius = cylinder_surface.radius().get();
            let offset_radius = cylinder_surface_2.radius().get();
            Some(offset_radius - support_radius)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface_2)),
        ) if {
            let support_axis = cone_surface.frame().axis().as_raw();
            let support_ref = cone_surface.frame().reference().as_raw();
            let support_ratio = cone_surface.ratio().get();
            let support_angle = cone_surface.half_angle().get();
            let offset_axis = cone_surface_2.frame().axis().as_raw();
            let offset_ref = cone_surface_2.frame().reference().as_raw();
            let offset_ratio = cone_surface_2.ratio().get();
            let offset_angle = cone_surface_2.half_angle().get();
            support_axis == offset_axis
                && support_ref == offset_ref
                && support_ratio.to_bits() == 1.0_f64.to_bits()
                && offset_ratio.to_bits() == 1.0_f64.to_bits()
                && support_angle.to_bits() == offset_angle.to_bits()
        } =>
        {
            let support_origin = cone_surface.origin();
            let support_axis = cone_surface.frame().axis().as_raw();
            let support_radius = cone_surface.radius().get();
            let support_angle = cone_surface.half_angle().get();
            let offset_origin = cone_surface_2.origin();
            let offset_radius = cone_surface_2.radius().get();
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let axial_delta = delta.dot(*support_axis);
            let residual = Vector3::new(
                delta.x - axial_delta * support_axis.x,
                delta.y - axial_delta * support_axis.y,
                delta.z - axial_delta * support_axis.z,
            );
            let radial_delta = offset_radius - support_radius;
            let distance = radial_delta * support_angle.cos() - axial_delta * support_angle.sin();
            let tangent_residual =
                radial_delta * support_angle.sin() + axial_delta * support_angle.cos();
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                support_radius,
                offset_radius,
                axial_delta,
                distance,
                tangent_residual,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (distance.is_finite()
                && residual.dot(residual) <= tolerance * tolerance
                && tangent_residual.abs() <= tolerance)
                .then_some(distance)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface_2)),
        ) if {
            let support_center = sphere_surface.center();
            let support_axis = sphere_surface.frame().axis().as_raw();
            let support_ref = sphere_surface.frame().reference().as_raw();
            let support_radius = sphere_surface.radius().get();
            let offset_center = sphere_surface_2.center();
            let offset_axis = sphere_surface_2.frame().axis().as_raw();
            let offset_ref = sphere_surface_2.frame().reference().as_raw();
            let offset_radius = sphere_surface_2.radius().get();
            support_center == offset_center
                && support_axis == offset_axis
                && support_ref == offset_ref
                && support_radius.signum().to_bits() == offset_radius.signum().to_bits()
        } =>
        {
            let support_radius = sphere_surface.radius().get();
            let offset_radius = sphere_surface_2.radius().get();
            Some((offset_radius - support_radius) * support_radius.signum())
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface_2)),
        ) if {
            let support_center = torus_surface.center();
            let support_axis = torus_surface.frame().axis().as_raw();
            let support_ref = torus_surface.frame().reference().as_raw();
            let support_major = torus_surface.major_radius().get();
            let support_minor = torus_surface.minor_radius().get();
            let offset_center = torus_surface_2.center();
            let offset_axis = torus_surface_2.frame().axis().as_raw();
            let offset_ref = torus_surface_2.frame().reference().as_raw();
            let offset_major = torus_surface_2.major_radius().get();
            let offset_minor = torus_surface_2.minor_radius().get();
            support_center == offset_center
                && support_axis == offset_axis
                && support_ref == offset_ref
                && support_major.to_bits() == offset_major.to_bits()
                && support_minor.signum().to_bits() == offset_minor.signum().to_bits()
                && support_major > support_minor.abs()
                && offset_major > offset_minor.abs()
        } =>
        {
            let support_minor = torus_surface.minor_radius().get();
            let offset_minor = torus_surface_2.minor_radius().get();
            Some((offset_minor - support_minor) * support_minor.signum())
        }
        _ => None,
    }
}

pub(super) fn blend_contact_offset_matches(
    support_offset: f64,
    spine_side_offset: f64,
    radius: f64,
) -> bool {
    let actual = (spine_side_offset - support_offset).abs();
    let expected = radius.abs();
    let scale = actual.max(expected).max(1.0);
    actual.is_finite()
        && expected.is_finite()
        && (actual - expected).abs() <= 64.0 * f64::EPSILON * scale
}

#[cfg(test)]
pub(super) fn surface_offset_lineage(
    ir: &CadIr,
    surface: &SurfaceId,
    depth: usize,
) -> Option<(SurfaceId, f64)> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, cadmpeg_ir::index::StandardIndex);
    surface_offset_lineage_with_index(
        &index,
        surface,
        depth,
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
    .map(|(base, distance)| (base.clone(), distance))
}

fn surface_offset_lineage_with_index<'a>(
    index: &'a cadmpeg_ir::index::ModelIndex<'_>,
    surface: &'a SurfaceId,
    depth: usize,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<(&'a SurfaceId, f64)>, cadmpeg_core::decode::ResourceLimit> {
    let _depth = ctx.enter_nested_limit("NX surface offset lineage depth")?;
    if depth >= 32 {
        return Ok(None);
    }
    if index.surfaces(surface.as_str(), ctx)?.is_none() {
        return Ok(None);
    }
    let Some(procedural) = index.procedural_surface_for_surface(surface.as_str(), ctx)? else {
        return Ok(Some((surface, 0.0)));
    };
    let ProceduralSurfaceDefinition::Offset(definition_payload) = procedural.definition() else {
        return Ok(Some((surface, 0.0)));
    };
    let support = definition_payload.support();
    let distance = definition_payload.distance().get();
    Ok(
        surface_offset_lineage_with_index(index, support, depth + 1, ctx)?
            .map(|(base, accumulated)| (base, accumulated + distance)),
    )
}

pub(super) struct CircularBlendDefinition<'a> {
    pub(super) supports: [&'a SurfaceId; 2],
    pub(super) spine: &'a CurveId,
    pub(super) radius: f64,
    pub(super) reversed: [bool; 2],
}

pub(super) fn blend_surface_definition_with_index<'a>(
    index: &'a cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<CircularBlendDefinition<'a>>, cadmpeg_core::decode::ResourceLimit> {
    let Some(procedural) = index.procedural_surface_for_surface(surface.as_str(), ctx)? else {
        return Ok(None);
    };
    Ok(blend_surface_definition_from_procedural(procedural))
}

fn blend_surface_definition_from_procedural(
    procedural: &ProceduralSurface,
) -> Option<CircularBlendDefinition<'_>> {
    let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() else {
        return None;
    };
    let (
        [Some(first), Some(second)],
        Some(spine),
        BlendRadiusLaw::Constant { signed_radius },
        BlendCrossSection::Circular,
    ) = (
        definition_payload.supports(),
        definition_payload.spine(),
        definition_payload.radius(),
        definition_payload.cross_section(),
    )
    else {
        return None;
    };

    let radius = signed_radius.get().abs();
    (radius > 0.0).then_some(CircularBlendDefinition {
        supports: [&first.surface, &second.surface],
        spine,
        radius,
        reversed: [first.reversed, second.reversed],
    })
}

#[cfg(test)]
pub(super) fn surface_contact_direction(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    center: Point3,
    radius: f64,
    depth: usize,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    surface_contact_direction_with_index(ctx, &index, surface, center, radius, depth)
}

#[cfg(test)]
fn surface_contact_direction_with_index(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    center: Point3,
    radius: f64,
    depth: usize,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    surface_contact_direction_with_index_and_budget(
        index,
        surface,
        center,
        radius,
        depth,
        &geometry_budget,
    )
}

fn surface_contact_direction_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    center: Point3,
    radius: f64,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    let ir = index.ir();
    if let Some(direction) = blend_surface_contact_direction_with_budget(
        index,
        surface,
        center,
        depth + 1,
        geometry_budget,
    )? {
        return Ok(Some(direction));
    }
    let Some(carrier) = index.surfaces(surface.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    let tolerance = ir.tolerances.linear.get();
    if !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let requires_radius_certificate = matches!(
        &carrier.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            | SurfaceGeometry::Procedural { .. }
    );
    let parameters = match carrier.geometry.solved() {
        Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            nurbs_surface_parameter_within_tolerance_with_budget(
                geometry_budget.charges,
                nurbs,
                center,
                None,
                radius + tolerance,
                geometry_budget,
            )?
            .map(cadmpeg_ir::units::FinitePoint2::get)
        }
        None => {
            let offset = offset_surface_parameters_with_tolerance_with_index_and_budget(
                index,
                surface,
                center,
                None,
                Some(radius + tolerance),
                geometry_budget,
            )?;
            if offset.is_some() {
                offset
            } else {
                blend_surface_parameters_inner(
                    index,
                    surface,
                    &BlendSurfaceFit {
                        point: center,
                        seed: None,
                        fit_tolerance: None,
                        grid: BlendParameterGrid::Disabled,
                        section_domain: BlendSectionDomain::Canonical,
                        depth: depth + 1,
                    },
                    geometry_budget,
                )?
            }
        }
        geometry => geometry
            .and_then(|geometry| {
                cadmpeg_ir::eval::analytic_surface_parameters_solved(geometry, center)
            })
            .map(Point2::from),
    };
    let Some(parameters) = parameters else {
        return Ok(None);
    };
    let Some(contact) = decoded_surface_point_inner_with_budget(
        index,
        surface,
        parameters.u,
        parameters.v,
        depth + 1,
        geometry_budget,
    )?
    else {
        return Ok(None);
    };
    let offset = Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    );
    Ok(
        (!requires_radius_certificate || (offset.norm() - radius).abs() <= tolerance)
            .then(|| FiniteVector3::new(offset).and_then(FiniteVector3::unit_nonzero))
            .flatten(),
    )
}

fn blend_surface_contact_direction_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    if depth >= 32 {
        return Ok(None);
    }
    let Some(CircularBlendDefinition { spine, .. }) =
        blend_surface_definition_with_index(index, surface, geometry_budget.charges)?
    else {
        return Ok(None);
    };
    let Some(u) =
        closest_spine_parameter_with_index_and_budget(index, spine, point, None, geometry_budget)?
    else {
        return Ok(None);
    };
    let Some(frame) =
        blend_surface_frame_with_index_and_budget(index, surface, u, depth + 1, geometry_budget)?
    else {
        return Ok(None);
    };
    let Some(radial) = FiniteVector3::new(Vector3::new(
        point.x - frame.0.x,
        point.y - frame.0.y,
        point.z - frame.0.z,
    ))
    .and_then(FiniteVector3::unit_nonzero) else {
        return Ok(None);
    };
    let sweep = signed_angle(frame.2, frame.3, frame.1);
    if !sweep.is_finite() || sweep.abs() <= EPS_BLEND_EXACT_GEOMETRY {
        return Ok(None);
    }
    let angle = signed_angle(frame.2, radial, frame.1);
    let Some(candidate) = (-2..=2)
        .map(|turn| (angle + f64::from(turn) * std::f64::consts::TAU) / sweep)
        .filter(|v| BlendSectionDomain::Canonical.contains(*v))
        .map(|v| blend_surface_point_from_frame(frame, v))
        .chain([
            blend_surface_point_from_frame(frame, 0.0),
            blend_surface_point_from_frame(frame, 1.0),
        ])
        .min_by(|first, second| {
            Point3::distance(*first, point).total_cmp(&Point3::distance(*second, point))
        })
    else {
        return Ok(None);
    };
    Ok(FiniteVector3::new(Vector3::new(
        candidate.x - point.x,
        candidate.y - point.y,
        candidate.z - point.z,
    ))
    .and_then(FiniteVector3::unit_nonzero))
}

fn model_curve_point_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    parameter: f64,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    let Some(carrier) = index.curves(curve.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
            .within_work_slice(geometry_budget, |admission| {
                cadmpeg_ir::eval::decode::curve_point(admission, &carrier.geometry, parameter)
            }),
    )
    .map(|point| point.map(cadmpeg_ir::features::FinitePoint3::get))
}

fn model_curve_tangent_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    parameter: f64,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Vector3>, cadmpeg_core::decode::ResourceLimit> {
    let Some(carrier) = index.curves(curve.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
            .within_work_slice(geometry_budget, |admission| {
                cadmpeg_ir::eval::decode::curve_tangent(admission, &carrier.geometry, parameter)
            }),
    )
    .map(|tangent| tangent.and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero))
}

#[cfg(test)]
pub(super) fn closest_spine_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    curve: &CurveId,
    point: Point3,
    seed: Option<f64>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    closest_spine_parameter_with_index_and_budget(&index, curve, point, seed, &geometry_budget)
}

pub(super) fn closest_spine_parameter_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    point: Point3,
    seed: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Some(carrier) = index.curves(curve.as_str(), geometry_budget.charges)? else {
        return Ok(None);
    };
    match carrier.geometry.solved() {
        Some(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            Ok(Some(
                (point.x - origin.x) * direction.x
                    + (point.y - origin.y) * direction.y
                    + (point.z - origin.z) * direction.z,
            ))
        }
        Some(geometry @ SolvedCurveGeometry::Circle(_)) => {
            closest_periodic_analytic_curve_parameter_with_budget(
                geometry,
                point,
                seed,
                geometry_budget,
            )
        }
        Some(geometry @ SolvedCurveGeometry::Ellipse(_)) => {
            closest_periodic_analytic_curve_parameter_with_budget(
                geometry,
                point,
                seed,
                geometry_budget,
            )
        }
        Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
            closest_nurbs_curve_parameter_with_budget(nurbs, point, seed, geometry_budget)
        }
        _ => Ok(None),
    }
}

fn closest_periodic_analytic_curve_parameter_with_budget(
    geometry: &SolvedCurveGeometry,
    point: Point3,
    seed: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    if seed.is_some_and(|seed| !seed.is_finite()) {
        return Ok(None);
    }
    let (center, axis, reference, ellipse) = match geometry {
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            (center, *axis, *ref_direction, None)
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            (center, *axis, *major_direction, Some(ellipse_curve))
        }
        _ => return Ok(None),
    };
    let transverse = axis.cross(reference);
    let delta = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let phase = delta.dot(transverse).atan2(delta.dot(reference));
    if !phase.is_finite() {
        return Ok(None);
    }
    let circle_parameter = seed.map_or(phase, |seed| {
        phase + ((seed - phase) / std::f64::consts::TAU).round() * std::f64::consts::TAU
    });
    if !circle_parameter.is_finite() {
        return Ok(None);
    }
    let Some(ellipse_curve) = ellipse else {
        return Ok(Some(circle_parameter));
    };
    let anchor = seed.unwrap_or(phase);
    let major_radius = ellipse_curve.major_radius().get();
    let minor_radius = ellipse_curve.minor_radius().get();
    let x = delta.dot(reference);
    let y = delta.dot(transverse);
    // The normal offset is constant over the ellipse. Normalize its planar
    // objective before forming products or ranking candidate parameters.
    let scale = major_radius.max(minor_radius).max(x.abs()).max(y.abs());
    let (major_radius, minor_radius, x, y) = (
        major_radius / scale,
        minor_radius / scale,
        x / scale,
        y / scale,
    );
    let difference = minor_radius * minor_radius - major_radius * major_radius;
    let coefficients = [
        -minor_radius * y,
        2.0 * (difference + major_radius * x),
        0.0,
        2.0 * (major_radius * x - difference),
        minor_radius * y,
    ];
    let constant_distance = coefficients.iter().all(|coefficient| *coefficient == 0.0);
    let roots = match real_polynomial_roots(geometry_budget.charges, &coefficients) {
        Ok(roots) => roots,
        Err(_) => return geometry_budget.resource_refusal().map_or(Ok(None), Err),
    };
    let Some(roots) = roots else {
        return Ok(None);
    };
    let parameters = roots
        .into_iter()
        .map(|root| 2.0 * root.atan())
        .chain([0.0, std::f64::consts::PI])
        .chain(constant_distance.then_some(anchor))
        .map(|parameter| {
            parameter
                + ((anchor - parameter) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        });
    // The quartic's root solver states a degree-bounded number of roots, so
    // the candidates, with the two ends and the anchor, have a fixed bound.
    let mut candidates = Vec::new();
    for parameter in parameters {
        if !geometry_budget.charge() {
            return geometry_budget.resource_refusal().map_or(Ok(None), Err);
        }
        candidates.push((
            parameter,
            (major_radius * parameter.cos() - x).hypot(minor_radius * parameter.sin() - y),
        ));
    }
    Ok(
        closest_parameter_candidates(&candidates, Some(anchor), geometry_budget)?
            .and_then(|parameters| parameters.first().copied()),
    )
}

/// The real roots of the quartic `coefficients`, lowest power first. A
/// quartic states a fixed amount of work: its roots, the roots of its
/// derivatives and their bisection steps are bounded by its degree. The
/// context admits the root sorts.
pub(super) fn real_polynomial_roots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    coefficients: &[f64; 5],
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    if coefficients
        .iter()
        .any(|coefficient| !coefficient.is_finite())
    {
        return Ok(None);
    }
    let Some(mut roots) = polynomial_roots_in_unit_interval(ctx, coefficients)? else {
        return Ok(None);
    };
    let mut reversed = *coefficients;
    reversed.reverse();
    let Some(reversed_roots) = polynomial_roots_in_unit_interval(ctx, &reversed)? else {
        return Ok(None);
    };
    roots.extend(
        reversed_roots
            .into_iter()
            .filter(|root| *root != 0.0)
            .map(f64::recip),
    );
    ctx.stable_sort_by(&mut roots, |value| value, f64::total_cmp, "nx polynomial roots sort")?;
    roots.dedup_by(|first, second| {
        (*first - *second).abs() <= 256.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Ok(Some(roots))
}

/// The roots in `[-1, 1]` of a polynomial of at most degree four, lowest
/// power first.
fn polynomial_roots_in_unit_interval(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    coefficients: &[f64],
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut coefficients = coefficients.to_vec();
    while coefficients
        .last()
        .is_some_and(|coefficient| *coefficient == 0.0)
    {
        coefficients.pop();
    }
    if coefficients.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let Some(degree) = coefficients.len().checked_sub(1) else {
        return Ok(None);
    };
    if degree == 0 {
        return Ok(Some(Vec::new()));
    }
    let scale = coefficients
        .iter()
        .fold(0.0_f64, |scale, coefficient| scale.max(coefficient.abs()));
    if !scale.is_finite() || scale == 0.0 {
        return Ok(Some(Vec::new()));
    }
    for coefficient in &mut coefficients {
        *coefficient /= scale;
    }
    if degree == 1 {
        let root = -coefficients[0] / coefficients[1];
        return Ok(root.is_finite().then(|| {
            if (-1.0..=1.0).contains(&root) {
                vec![root]
            } else {
                Vec::new()
            }
        }));
    }
    let Some(derivative) = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .map(|(degree, coefficient)| {
            Some(*coefficient * cadmpeg_core::convert::f64_from_index(degree)?)
        })
        .collect::<Option<Vec<_>>>() else {
        return Ok(None);
    };
    let Some(mut critical) = polynomial_roots_in_unit_interval(ctx, &derivative)? else {
        return Ok(None);
    };
    ctx.stable_sort_by(&mut critical, |value| value, f64::total_cmp, "nx polynomial critical roots sort")?;
    critical.dedup_by(|first, second| {
        (*first - *second).abs() <= 64.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    let value = |parameter| polynomial_value(&coefficients, parameter);
    let tolerance = |parameter: f64| {
        256.0
            * f64::EPSILON
            * coefficients.iter().rev().fold(0.0, |bound, coefficient| {
                bound * parameter.abs() + coefficient.abs()
            })
    };
    let mut roots = critical
        .iter()
        .copied()
        .filter(|root| value(*root).abs() <= tolerance(*root))
        .collect::<Vec<_>>();
    let partitions = std::iter::once(-1.0)
        .chain(critical)
        .chain(std::iter::once(1.0))
        .collect::<Vec<_>>();
    for pair in partitions.windows(2) {
        let mut lower = pair[0];
        let mut upper = pair[1];
        let mut lower_value = value(lower);
        let upper_value = value(upper);
        if lower_value.abs() <= tolerance(lower) {
            roots.push(lower);
            continue;
        }
        if upper_value.abs() <= tolerance(upper) {
            roots.push(upper);
            continue;
        }
        if lower_value.is_sign_positive() == upper_value.is_sign_positive() {
            continue;
        }
        for _ in 0..128 {
            let middle = lower + (upper - lower) * 0.5;
            if middle == lower || middle == upper {
                break;
            }
            let middle_value = value(middle);
            if middle_value.abs() <= tolerance(middle) {
                lower = middle;
                upper = middle;
                break;
            }
            if middle_value.is_sign_positive() == lower_value.is_sign_positive() {
                lower = middle;
                lower_value = middle_value;
            } else {
                upper = middle;
            }
        }
        roots.push(lower + (upper - lower) * 0.5);
    }
    ctx.stable_sort_by(&mut roots, |value| value, f64::total_cmp, "nx polynomial roots sort")?;
    roots.dedup_by(|first, second| {
        (*first - *second).abs() <= 256.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Ok(Some(roots))
}

fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |value, coefficient| value * parameter + coefficient)
}

pub(super) fn closest_nurbs_curve_parameter_with_budget(
    curve: &NurbsCurve,
    point: Point3,
    seed: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let count = curve.pole_rows().count();
    if !point.is_finite() {
        return Ok(None);
    }
    let (Some(lower), Some(upper)) = (curve.knots().get(degree), curve.knots().get(count)) else {
        return Ok(None);
    };
    let domain = [*lower, *upper];
    if domain[0] >= domain[1] || seed.is_some_and(|seed| !seed.is_finite()) {
        return Ok(None);
    }
    let ctx = geometry_budget.charges;
    let search_seed = seed.map(|seed| canonical_periodic_parameter(domain, curve.periodic(), seed));
    let poles = curve.pole_rows();
    let rational = poles.weight_at(0).is_some();
    let mut weights = ScopedValues::with_capacity(
        ctx,
        if rational { count } else { 0 },
        "nx spine NURBS weights",
    )?;
    let mut residuals = ScopedValues::with_capacity(ctx, count, "nx spine NURBS residuals")?;
    let mut coordinate_scale = [point.x, point.y, point.z]
        .into_iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    // One pass reads each pole's weight and position in place.
    for index in ctx.admit_iter(&(0..count), "nx spine NURBS poles")? {
        if rational {
            let Some(weight) = poles.weight_at(index).filter(|weight| *weight > 0.0) else {
                return Ok(None);
            };
            weights.values.push(weight);
        }
        let Some(control) = poles.point_at(index) else {
            return Ok(None);
        };
        coordinate_scale = [control.x, control.y, control.z]
            .into_iter()
            .fold(coordinate_scale, |scale, value| scale.max(value.abs()));
        residuals.values.push(Point3::new(
            control.x - point.x,
            control.y - point.y,
            control.z - point.z,
        ));
    }
    let Some(controls) = positive_controls(
        ctx,
        &residuals,
        rational.then_some(&*weights),
        "nx spine positive controls",
    )?
    else {
        return Ok(None);
    };
    let Some(spans) = homogeneous_spans(geometry_budget.charges, degree, curve.knots(), &controls)?
    else {
        return Ok(None);
    };
    let homogeneous = HomogeneousCurveSpans {
        extraction: spans,
        coordinate_tolerance: 64.0 * f64::EPSILON * coordinate_scale,
    };
    let Some(candidates) =
        stationary_rational_distance_candidates(&homogeneous, search_seed, geometry_budget)?
    else {
        return geometry_budget.resource_refusal().map_or(Ok(None), Err);
    };
    let Some(parameters) = closest_parameter_candidates(&candidates, search_seed, geometry_budget)?
    else {
        return geometry_budget.resource_refusal().map_or(Ok(None), Err);
    };
    nearest_lifted_periodic_parameter(ctx, &parameters, domain, curve.periodic(), seed)
}

fn signed_angle(first: Vector3, second: Vector3, axis: Vector3) -> f64 {
    first.cross(second).dot(axis).atan2(first.dot(second))
}

fn rodrigues_rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cross = axis.cross(vector);
    let dot = axis.dot(vector);
    Vector3::new(
        vector.x * angle.cos() + cross.x * angle.sin() + axis.x * dot * (1.0 - angle.cos()),
        vector.y * angle.cos() + cross.y * angle.sin() + axis.y * dot * (1.0 - angle.cos()),
        vector.z * angle.cos() + cross.z * angle.sin() + axis.z * dot * (1.0 - angle.cos()),
    )
}

#[cfg(test)]
mod numerical_range_tests;

#[cfg(test)]
mod context_tests;
