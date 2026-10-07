// SPDX-License-Identifier: Apache-2.0
use super::super::{
    cluster_boundary_positions, BoundarySurfaceKind, BoundaryVertexClusterError,
    BoundaryVertexCreationError, NonSimpleRing, SimpleRing,
};
use cadmpeg_core::{
    decode::{DecodePolicy, ResourceDimension},
    CodecError,
};
use cadmpeg_ir::math::Point3;

fn square() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]]
}

#[test]
fn simple_ring_duplicate_proof_refuses_work() {
    assert_scan_work_refusal("iges closed polyline duplicate comparisons", |ctx| {
        SimpleRing::new(square(), ctx)
    });
}

#[test]
fn simple_ring_intersection_proof_refuses_work() {
    assert_scan_work_refusal("iges planar self-intersection comparisons", |ctx| {
        SimpleRing::new(square(), ctx)
    });
}

#[test]
fn trim_relationship_propagates_intersection_work_refusal() {
    let rings = crate::test_support::with_service_context(&[], |ctx| {
        vec![
            SimpleRing::new(square(), ctx).unwrap().unwrap(),
            SimpleRing::new(
                vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [1.0, 1.0]],
                ctx,
            )
            .unwrap()
            .unwrap(),
        ]
    });
    let plane = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
    );
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges planar ring intersection comparisons",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::linear_boundary_relationship_is_valid(
                    Ok(&rings),
                    BoundarySurfaceKind::Trimmed,
                    true,
                    &plane,
                    None,
                    [false, false],
                    ctx,
                )
            })
        },
    );
    crate::test_support::with_service_context(&[], |ctx| {
        assert_eq!(
            super::super::linear_boundary_relationship_is_valid(
                Ok(&rings),
                BoundarySurfaceKind::Trimmed,
                true,
                &plane,
                None,
                [false, false],
                ctx
            )
            .unwrap(),
            Some(true)
        );
        assert_eq!(
            super::super::linear_boundary_relationship_is_valid(
                Err(&NonSimpleRing),
                BoundarySurfaceKind::Trimmed,
                true,
                &plane,
                None,
                [false, false],
                ctx
            )
            .unwrap(),
            Some(false)
        );
    });
}

#[test]
fn trim_containment_proof_refuses_work() {
    let ring = crate::test_support::with_service_context(&[], |ctx| {
        SimpleRing::new(square(), ctx).unwrap().unwrap()
    });
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges planar point containment comparisons",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::planar_point_is_strictly_inside([1.0, 1.0], &ring, ctx)
            })
        },
    );
}

#[test]
fn implicit_outer_ring_relationship_propagates_pair_work_refusal() {
    let rings = crate::test_support::with_service_context(&[], |ctx| {
        vec![
            SimpleRing::new(square(), ctx).unwrap().unwrap(),
            SimpleRing::new(
                square().into_iter().map(|[x, y]| [x + 5.0, y]).collect(),
                ctx,
            )
            .unwrap()
            .unwrap(),
        ]
    });
    let plane = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
    );
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges planar ring intersection comparisons",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::linear_boundary_relationship_is_valid(
                    Ok(&rings),
                    BoundarySurfaceKind::Trimmed,
                    false,
                    &plane,
                    None,
                    [false, false],
                    ctx,
                )
            })
        },
    );
}

#[test]
fn boundary_clustering_propagates_root_work_refusal() {
    let points = [
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .unwrap(),
    ];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges boundary cluster root traversal",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                let result = super::super::cluster_boundary_positions(
                    &points,
                    cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
                    ctx,
                );
                match result {
                    Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(
                        limit,
                    ))) => {
                        assert_eq!(ctx.resource_refusal(), Some(limit));
                        Err(CodecError::ResourceLimit(limit))
                    }
                    Err(error) => panic!("unexpected cluster refusal: {error:?}"),
                    Ok(_) => Ok(()),
                }
            })
        },
    );
}

fn assert_scan_work_refusal<T>(
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| run(ctx))
        },
    );
}

#[test]
fn pcurve_knot_insertion_refuses_work_before_shift() {
    let controls = [[1.0, 0.0, 0.0, 0.0]; 4];
    let knots = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
    assert_scan_work_refusal("iges pcurve inserted knots", |ctx| {
        super::super::homogeneous_pcurve_spans(2, &knots, controls.to_vec(), ctx)
    });
}

#[test]
fn pcurve_split_refuses_work_before_interpolation() {
    let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
    assert_scan_work_refusal("iges pcurve split interpolation", |ctx| {
        super::super::split_homogeneous_pcurve(&controls, 0.5, ctx)
    });
}

#[test]
fn boundary_clustering_chain_uses_no_call_stack_depth() {
    let m = 256_u32;
    let points: Vec<_> = (0..=m)
        .map(|i| f64::from(i) * 1.5)
        .chain((0..m).rev().map(|i| f64::from(i) * 1.5 + 0.75))
        .map(|x| cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap())
        .collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(matches!(
            cluster_boundary_positions(
                &points,
                cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
                ctx
            ),
            Err(BoundaryVertexCreationError::Cluster(
                BoundaryVertexClusterError::NonTransitive
            ))
        ));
    });
}

#[test]
fn boundary_clustering_root_walk_refuses_before_traversal() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges boundary cluster root traversal",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::find_cluster_root(&mut [0], 0, ctx)
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == 0 && limit.additional == 1));
}

#[test]
fn native_identity_sequence_parse_refuses_work() {
    assert_scan_work_refusal("iges native identity sequence", |ctx| {
        super::super::native_sequence_from_id("iges:model:surface#D1", "iges:model:surface#D", ctx)
    });
}

#[test]
fn support_interval_identity_parse_refusal_precedes_missing_record_fallback() {
    let id = cadmpeg_ir::ids::SurfaceId::mint("iges:model:surface#D1").unwrap();
    assert_scan_work_refusal("iges native identity sequence", |ctx| {
        super::super::surface_parameter_bound_intervals(
            Some([None; 4]),
            &id,
            &[],
            &[],
            crate::global::RealPrecision {
                single_significance: 7,
                double_significance: 15,
            },
            ctx,
        )
    });
}

#[test]
fn pcurve_knot_runs_preserve_quadratic_span_controls() {
    let knots = [0.0, 0.0, 0.0, 0.25, 0.75, 1.0, 1.0, 1.0];
    let controls = (0..5)
        .map(|value| [1.0, f64::from(value), 0.0, 0.0])
        .collect();
    crate::test_support::with_service_context(&[], |ctx| {
        let spans = super::super::homogeneous_pcurve_spans(2, &knots, controls, ctx)
            .unwrap()
            .unwrap();
        assert_eq!(spans.len(), 3);
        let expected = [
            ([0.0, 0.25], [0.0, 1.0, 4.0 / 3.0]),
            ([0.25, 0.75], [4.0 / 3.0, 2.0, 8.0 / 3.0]),
            ([0.75, 1.0], [8.0 / 3.0, 3.0, 4.0]),
        ];
        for (span, (domain, ordinates)) in spans.iter().zip(expected) {
            assert_eq!(span.domain, domain);
            assert_eq!(span.controls.len(), 3);
            for (control, ordinate) in span.controls.iter().zip(ordinates) {
                assert_eq!(control[0], 1.0);
                assert!((control[1] - ordinate).abs() <= 4.0 * f64::EPSILON);
                assert_eq!(&control[2..], &[0.0, 0.0]);
            }
        }
    });
}

#[test]
fn pcurve_knot_validation_stops_at_the_first_invalid_value() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let knots = [f64::NAN, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
        assert!(super::super::homogeneous_pcurve_spans(
            2,
            &knots,
            vec![[1.0, 0.0, 0.0, 0.0]; 4],
            ctx,
        )
        .unwrap()
        .is_none());
    });
}

#[test]
fn source_curve_active_identity_uses_scoped_storage() {
    use cadmpeg_ir::codec::Codec;
    let bytes = super::subrange_nurbs_surface_boundary_file_with_source_precision_outside_nominal();
    let decoded = super::IgesCodec
        .decode(
            &mut std::io::Cursor::new(&bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap();
    let index =
        cadmpeg_ir::index::ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    let id = cadmpeg_ir::ids::CurveId::mint("iges:model:curve#D5").unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges source active curve ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                let mut scratch = ctx.reserve_scoped(0, "test source intervals")?;
                scratch.with_storage(|| {
                    super::super::source_curve_control_intervals(
                        &index,
                        &id,
                        (&[], &[]),
                        crate::global::RealPrecision {
                            single_significance: 7,
                            double_significance: 15,
                        },
                        1.0,
                        &mut std::collections::BTreeSet::new(),
                        ctx,
                    )
                })
            })
        },
    );
}

#[test]
fn pcurve_endpoint_agreement_reaches_evaluation_without_mapping_storage() {
    let ir = cadmpeg_ir::CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#absent").unwrap();
    let pcurves = [(
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        [0.0, 1.0],
    )];
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "model evaluation cycle path",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::pcurves_agree(
                    &index,
                    &id,
                    &pcurves,
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    super::EPS_BOUNDARY_ENDPOINT_MATCH,
                    ctx,
                )
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == 0 && limit.additional == 1));
    crate::test_support::with_service_context(&[], |ctx| {
        assert!(!super::super::pcurves_agree(
            &index,
            &id,
            &pcurves,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            super::EPS_BOUNDARY_ENDPOINT_MATCH,
            ctx,
        )
        .unwrap());
    });
}

#[test]
fn pcurve_knot_runs_preserve_cubic_repeated_insertions() {
    let knots = [-1.0, -1.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let controls = (0..5)
        .map(|value| [1.0, f64::from(value), 0.0, 0.0])
        .collect();
    crate::test_support::with_service_context(&[], |ctx| {
        let spans = super::super::homogeneous_pcurve_spans(3, &knots, controls, ctx)
            .unwrap()
            .unwrap();
        assert_eq!(spans.len(), 2);
        for (span, (domain, ordinates)) in spans.iter().zip([
            ([-1.0, 0.0], [0.0, 1.0, 1.5, 2.0]),
            ([0.0, 1.0], [2.0, 2.5, 3.0, 4.0]),
        ]) {
            assert_eq!(span.domain, domain);
            assert_eq!(span.controls.len(), 4);
            for (control, ordinate) in span.controls.iter().zip(ordinates) {
                assert_eq!(*control, [1.0, ordinate, 0.0, 0.0]);
            }
        }
    });
}

#[test]
fn pcurve_knot_runs_keep_the_total_order_representative_of_signed_zero() {
    let knots = [-1.0, -1.0, -1.0, -1.0, 0.0, -0.0, 1.0, 1.0, 1.0, 1.0];
    crate::test_support::with_service_context(&[], |ctx| {
        let spans =
            super::super::homogeneous_pcurve_spans(3, &knots, vec![[1.0, 0.0, 0.0, 0.0]; 6], ctx)
                .unwrap()
                .unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].domain, [-0.0, 1.0]);
        assert!(spans[1].domain[0].is_sign_negative());
    });
}

#[test]
fn pcurve_split_keeps_one_scratch_level_and_exact_edge_controls() {
    let controls = [
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 1.0, 0.0, 0.0],
        [1.0, 2.0, 0.0, 0.0],
        [1.0, 3.0, 0.0, 0.0],
    ];
    let scratch_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of_val(&controls));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2 * scratch_bytes;
    policy.limits.max_materialized_bytes = scratch_bytes;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let (left, right) = super::super::split_homogeneous_pcurve(&controls, 0.5, ctx)
            .unwrap()
            .unwrap();
        assert_eq!(
            left,
            [
                [1.0, 0.0, 0.0, 0.0],
                [1.0, 0.5, 0.0, 0.0],
                [1.0, 1.0, 0.0, 0.0],
                [1.0, 1.5, 0.0, 0.0],
            ]
        );
        assert_eq!(
            right,
            [
                [1.0, 1.5, 0.0, 0.0],
                [1.0, 2.0, 0.0, 0.0],
                [1.0, 2.5, 0.0, 0.0],
                [1.0, 3.0, 0.0, 0.0],
            ]
        );
        ctx.reserve_scoped(scratch_bytes, "released pcurve split working storage")
            .unwrap();
    });
}

#[test]
fn polynomial_boundaries_skip_implicit_weight_visits() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsCurve, NurbsPoles3},
        pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles},
        Curve, CurveGeometry,
    };
    let positions = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let nurbs = crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::new(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            NurbsPoles3::Polynomial {
                points: positions
                    .map(|point| cadmpeg_ir::features::FinitePoint3::new(point).unwrap())
                    .to_vec(),
            },
            false,
        )
        .unwrap()
        .unwrap()
    });
    let pcurve = crate::test_support::with_service_context(&[], |ctx| {
        PcurveNurbs::new(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            PcurveNurbsPoles::Polynomial {
                points: [[0.0, 0.0], [1.0, 0.0]]
                    .map(|[u, v]| {
                        cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(u, v))
                            .unwrap()
                    })
                    .to_vec(),
            },
            false,
        )
        .unwrap()
        .unwrap()
    });
    let id = cadmpeg_ir::ids::CurveId::mint("test:model:curve#linear").unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(nurbs)) =
        ir.model.curves[0].geometry.solved()
    else {
        panic!("polynomial NURBS carrier");
    };
    let pcurve = PcurveGeometry::Nurbs { nurbs: pcurve };
    for case in 0..3 {
        let operation = if case == 2 {
            "iges source positive weights"
        } else {
            "iges linear boundary weights"
        };
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_policy_context(&[], &policy, |ctx| match case {
            0 => assert_eq!(
                super::super::linear_model_nurbs_points(nurbs, [0.0, 1.0], ctx)
                    .unwrap()
                    .unwrap(),
                positions
            ),
            1 => assert_eq!(
                super::super::linear_pcurve_points(&pcurve, [0.0, 1.0], ctx)
                    .unwrap()
                    .unwrap(),
                [[0.0, 0.0], [1.0, 0.0]]
            ),
            _ => {
                let controls = super::super::source_curve_control_intervals(
                    &index,
                    &id,
                    (&[], &[]),
                    crate::global::RealPrecision {
                        single_significance: 7,
                        double_significance: 15,
                    },
                    1.0,
                    &mut std::collections::BTreeSet::new(),
                    ctx,
                )
                .unwrap()
                .unwrap();
                assert_eq!(
                    controls
                        .iter()
                        .map(|control| control
                            .map(|interval| [interval.lower_bound(), interval.upper_bound()]))
                        .collect::<Vec<_>>(),
                    positions.map(|point| [point.x, point.y, point.z].map(|value| [value, value])),
                );
            }
        });
    }
}

#[test]
fn ambiguous_boundary_selection_uses_no_candidate_collection_slots() {
    use cadmpeg_ir::codec::Codec;
    let decoded = super::IgesCodec
        .decode(
            &mut std::io::Cursor::new(super::bounded_plane_file()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap();
    let index =
        cadmpeg_ir::index::ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    let first = &decoded.ir().model.edges[0];
    let mut second = first.clone();
    second.id = cadmpeg_ir::ids::EdgeId::mint("test:model:edge#second").unwrap();
    let candidates = [first, &second];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(matches!(
            super::super::select_boundary_edge(
                &candidates,
                &index,
                super::super::BoundaryMatch {
                    surface_id: &decoded.ir().model.surfaces[0].id,
                    pcurves: &[],
                    sense: cadmpeg_ir::topology::Sense::Forward,
                    tolerance: super::EPS_BOUNDARY_ENDPOINT_MATCH,
                    parameter_curves_authoritative: false,
                },
                ctx,
            ),
            Err(super::super::BoundaryEdgeSelectionError::Ambiguous)
        ));
    });
}

#[test]
fn pcurve_endpoint_agreement_stops_before_a_rejected_end_evaluation() {
    use cadmpeg_ir::codec::Codec;
    let decoded = super::IgesCodec
        .decode(
            &mut std::io::Cursor::new(super::bounded_plane_file()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap();
    let index =
        cadmpeg_ir::index::ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    let pcurves = [(
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        [0.0, 1.0],
    )];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(!super::super::pcurves_agree(
            &index,
            &decoded.ir().model.surfaces[0].id,
            &pcurves,
            Point3::new(4.0, 4.0, 4.0),
            Point3::new(1.0, 0.0, 0.0),
            super::EPS_BOUNDARY_ENDPOINT_MATCH,
            ctx,
        )
        .unwrap());
    });
}
