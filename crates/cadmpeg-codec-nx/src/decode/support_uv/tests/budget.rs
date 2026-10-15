// SPDX-License-Identifier: Apache-2.0
//! Support assignment, traversal and retry-state resource boundaries.

use super::super::*;
use crate::test_support::{resource_refusal_at, with_decode_context, with_decode_context_over};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Vector3;

fn plane_model() -> (CadIr, [SurfaceId; 2]) {
    let ids = [
        SurfaceId::mint("test:model:surface#first").unwrap(),
        SurfaceId::mint("test:model:surface#second").unwrap(),
    ];
    let mut ir = CadIr::empty();
    for (i, z) in [0.0, 1.0].into_iter().enumerate() {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: ids[i].clone(),
            source_object: None,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, z),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
        });
    }
    (ir, ids)
}

fn charted_fixture() -> (CadIr, Vec<PendingExt11SupportUv>) {
    use crate::test_support::test_streams::{
        prt_with_ext11_intersection,
        two_support_charted_intersection_curve_stream_with_second_plane_axis,
        two_support_ext11_charted_intersection_curve_stream,
    };
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let result = crate::NxCodec
        .decode(
            &mut std::io::Cursor::new(prt_with_ext11_intersection(&partition, &stream)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = result.ir().clone();
    let pending = vec![(
        ir.model.procedural_curves[0].id.clone(),
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            vec![0.0, 0.01],
        )
        .unwrap(),
        0.01,
        SerializedSupportUv::default(),
    )];
    (ir, pending)
}

fn long_pcurve(count: usize) -> PcurveGeometry {
    with_decode_context(|ctx| {
        let mut knots = vec![0.0];
        knots.extend((0..count).map(|i| cadmpeg_core::convert::f64_from_index(i).unwrap()));
        knots.push(cadmpeg_core::convert::f64_from_index(count - 1).unwrap());
        PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                ctx,
                1,
                knots,
                (0..count)
                    .map(|i| Point2::new(cadmpeg_core::convert::f64_from_index(i).unwrap(), 0.0))
                    .collect(),
                None,
                false,
            )
            .unwrap()
            .unwrap(),
        }
    })
}

#[test]
fn accepted_ext11_assignment_borrows_lanes_without_retained_storage() {
    let (ir, ids) = plane_model();
    let index =
        cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)];
    let lanes = [SupportUvLane::new(vec![[0.0, 0.0], [0.01, 0.0]], 2), None];
    with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let geometry = GeometryWorkBudget::from_context(ctx, 10000);
            for _ in 0..32 {
                let assigned = assign_ext11_support_uv_to_surfaces_with_index(
                    ctx,
                    &index,
                    [&ids[0], &ids[1]],
                    &points,
                    0.01,
                    &lanes,
                    &geometry,
                )
                .unwrap()
                .unwrap();
                assert!(std::ptr::eq(
                    assigned[0].unwrap(),
                    lanes[0].as_ref().unwrap()
                ));
                assert!(assigned[1].is_none());
            }
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn ext11_fit_refuses_at_the_visit_and_does_not_charge_rejected_suffixes() {
    let (ir, ids) = plane_model();
    let index =
        cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let points = vec![Point3::new(0.0, 0.0, 0.0); 32];
    for first in [[f64::MAX, 0.0], [1.0, 0.0]] {
        let mut values = vec![[0.0, 0.0]; 32];
        values[0] = first;
        let lane = SupportUvLane::new(values, 32).unwrap();
        let error = resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "nx support UV lane fit traversal",
            |ctx| {
                let geometry = GeometryWorkBudget::from_context(ctx, 10000);
                Ok(support_uv_lane_matches_surface_with_budget(
                    &index,
                    &ids[0],
                    &points,
                    0.01,
                    Some(&lane),
                    &geometry,
                )?)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("fit visit refusal")
        };
        assert_eq!(limit.additional, 1);
        // One early-rejected sample has the same cost regardless of suffix length.
        let work = |count| {
            with_decode_context(|ctx| {
                let geometry = GeometryWorkBudget::from_context(ctx, 10000);
                let lane = SupportUvLane::new(vec![first; count], count).unwrap();
                assert!(!support_uv_lane_matches_surface_with_budget(
                    &index,
                    &ids[0],
                    &points[..count],
                    0.01,
                    Some(&lane),
                    &geometry
                )
                .unwrap());
                let CodecError::ResourceLimit(usage) = ctx
                    .charge_work(u64::MAX, "nx test work census")
                    .unwrap_err()
                else {
                    panic!("work census")
                };
                usage.used
            })
        };
        assert_eq!(work(1), work(32));
    }
}

#[test]
fn validated_candidates_do_not_materialize_chart_samples() {
    let (ir, pending) = charted_fixture();
    let validated = BTreeSet::from([(pending[0].0.clone(), 0), (pending[0].0.clone(), 1)]);
    for repeated in [1, 32] {
        let pending = vec![pending[0].clone(); repeated];
        let run = |cap| {
            with_decode_context_over(
                &[],
                |policy| policy.limits.max_materialized_bytes = cap,
                |ctx| {
                    let mut ir = ir.clone();
                    let support = WorkBudget::new(1000);
                    let geometry = GeometryWorkBudget::from_context(ctx, 100_000);
                    let mut storage = ctx.reserve_scoped(0, "nx test witnesses")?;
                    let mut witnesses = BTreeMap::new();
                    invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
                        ctx,
                        &mut ir,
                        &pending,
                        &validated,
                        (&support, &geometry),
                        false,
                        &mut EndpointWitnessSink::new(&mut witnesses, &mut storage),
                    )?;
                    assert!(witnesses.is_empty());
                    assert_eq!(support.consumed(), 0);
                    Ok::<_, CodecError>(())
                },
            )
        };
        // Discover only setup reservations. No chart vector may be requested.
        let mut cap = 0;
        loop {
            match run(cap) {
                Ok(()) => break,
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_ne!(limit.operation, "NX chart points");
                    assert_ne!(limit.operation, "NX chart parameters");
                    cap = limit.used + limit.additional;
                }
                Err(error) => panic!("{error}"),
            }
        }
    }
}

#[test]
fn depleted_support_budgets_leave_pending_suffixes_unvisited() {
    let (ir, pending) = charted_fixture();
    for phase in ["validation", "direct", "coupled"] {
        let work = |count| {
            with_decode_context(|ctx| {
                let pending = vec![pending[0].clone(); count];
                let mut ir = ir.clone();
                let support = WorkBudget::new(if phase == "direct" { 2 } else { 0 });
                let empty_support = WorkBudget::new(0);
                let geometry = GeometryWorkBudget::from_context(ctx, 100_000);
                let mut witnesses = BTreeMap::new();
                let mut storage = ctx.reserve_scoped(0, "nx test witnesses").unwrap();
                let mut sink = EndpointWitnessSink::new(&mut witnesses, &mut storage);
                if phase == "direct" {
                    ir.model.procedural_curves[0]
                        .intersection_context_mut()
                        .unwrap()
                        .set_unmapped_pcurve(0, None);
                    let mut direct_storage =
                        ctx.reserve_scoped(0, "nx test direct retry table").unwrap();
                    let mut direct = FailedSupportUvAttempts::new();
                    let mut coupled_storage = ctx
                        .reserve_scoped(0, "nx test coupled retry table")
                        .unwrap();
                    let mut coupled = FailedCoupledSupportUvAttempts::new();
                    complete_support_uv_wave(
                        ctx,
                        &mut ir,
                        SupportUvAttempts {
                            pending: &pending,
                            failed_attempts: &mut direct,
                            failed_storage: &mut direct_storage,
                            failed_coupled_attempts: &mut coupled,
                            failed_coupled_storage: &mut coupled_storage,
                            endpoint_witnesses: &mut sink,
                        },
                        &support,
                        &geometry,
                        &empty_support,
                        &geometry,
                    )
                    .unwrap();
                    assert_eq!(support.consumed(), 2);
                    let ProceduralCurveDefinition::Intersection { context, .. } =
                        ir.model.procedural_curves[0].definition()
                    else {
                        panic!("charted intersection");
                    };
                    assert!(context.sides()[0].pcurve.is_some());
                } else if phase == "coupled" {
                    let mut retry_storage = ctx
                        .reserve_scoped(0, "nx test coupled retry table")
                        .unwrap();
                    let mut retries = FailedCoupledSupportUvAttempts::new();
                    complete_coupled_support_uv(
                        ctx,
                        &mut ir,
                        &pending,
                        &support,
                        &geometry,
                        (&mut retries, &mut retry_storage),
                        &mut sink,
                    )
                    .unwrap();
                } else {
                    invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
                        ctx,
                        &mut ir,
                        &pending,
                        &BTreeSet::new(),
                        (&support, &geometry),
                        false,
                        &mut sink,
                    )
                    .unwrap();
                }
                let CodecError::ResourceLimit(usage) = ctx
                    .charge_work(u64::MAX, "nx test work census")
                    .unwrap_err()
                else {
                    panic!("work census")
                };
                usage.used
            })
        };
        assert_eq!(work(1), work(64));
    }
}

#[test]
fn validation_publishes_a_witness_when_the_sample_budget_fits_exactly() {
    let (ir, pending) = charted_fixture();
    with_decode_context(|ctx| {
        let mut ir = ir.clone();
        let support = WorkBudget::new(2);
        let geometry = GeometryWorkBudget::from_context(ctx, 100_000);
        let mut storage = ctx.reserve_scoped(0, "nx test witnesses").unwrap();
        let mut witnesses = BTreeMap::new();
        invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
            ctx,
            &mut ir,
            &pending,
            &BTreeSet::new(),
            (&support, &geometry),
            false,
            &mut EndpointWitnessSink::new(&mut witnesses, &mut storage),
        )
        .unwrap();
        assert_eq!(support.consumed(), 2);
        assert!(!witnesses.is_empty());
    });
}

#[test]
fn retry_states_release_replaced_payloads_and_use_no_retained_bytes() {
    let source = long_pcurve(1024);
    with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 131_072;
        },
        |ctx| {
            let mut storage = ctx.reserve_scoped(0, "nx support UV retry table").unwrap();
            let mut attempts = FailedSupportUvAttempts::new();
            for _ in 0..64 {
                record_failed_support_uv_attempt(
                    ctx,
                    &mut attempts,
                    &mut storage,
                    ("test:model:curve#retry", 0),
                    Some(&source),
                )
                .unwrap();
            }
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts.values().next().unwrap().0.as_ref(), Some(&source));
            drop(attempts);
            drop(storage);
            // Both the node reservation and its final payload lease are released.
            ctx.reserve_scoped(131_072, "nx test retry release proof")
                .unwrap();
        },
    );
    resource_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "nx support UV retry pcurve",
        |ctx| {
            let mut storage = ctx.reserve_scoped(0, "nx support UV retry table")?;
            let mut attempts = FailedSupportUvAttempts::new();
            record_failed_support_uv_attempt(
                ctx,
                &mut attempts,
                &mut storage,
                ("test:model:curve#retry", 0),
                Some(&source),
            )
        },
    );
}

#[test]
fn retry_routes_admit_geometry_comparisons_after_the_lookup() {
    let (mut ir, pending) = charted_fixture();
    let source = long_pcurve(64);
    ir.model.procedural_curves[0]
        .intersection_context_mut()
        .unwrap()
        .set_unmapped_pcurve(0, None);
    ir.model.procedural_curves[0]
        .intersection_context_mut()
        .unwrap()
        .set_unmapped_pcurve(1, Some(source.clone()));
    for coupled in [false, true] {
        let operation = if coupled {
            "nx coupled support UV retry pcurve comparison"
        } else {
            "nx support UV retry pcurve comparison"
        };
        resource_refusal_at(&[], ResourceDimension::WorkUnits, operation, |ctx| {
            let mut ir = ir.clone();
            let support = WorkBudget::new(1000);
            let empty_support = WorkBudget::new(0);
            let geometry = GeometryWorkBudget::from_context(ctx, 100_000);
            let mut witness_storage = ctx.reserve_scoped(0, "nx test witnesses")?;
            let mut witnesses = BTreeMap::new();
            let mut sink = EndpointWitnessSink::new(&mut witnesses, &mut witness_storage);
            let mut direct_storage = ctx.reserve_scoped(0, "nx support UV retry table")?;
            let mut direct = FailedSupportUvAttempts::new();
            let mut coupled_storage = ctx.reserve_scoped(0, "nx coupled support UV retry table")?;
            let mut coupled_attempts = FailedCoupledSupportUvAttempts::new();
            if coupled {
                let (state, state_storage) =
                    ctx.with_scoped_storage("nx coupled support UV retry state", || {
                        Ok::<_, CodecError>([
                            None,
                            Some(cadmpeg_ir::geometry::SupportPcurve::new(
                                source.try_clone_for_decode(
                                    ctx,
                                    "nx coupled support UV retry pcurve",
                                )?,
                                None,
                            )),
                        ])
                    })?;
                drop(coupled_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut coupled_attempts,
                        pending[0].0.as_str(),
                        (state, state_storage),
                        "nx coupled support UV failed retries",
                    )
                })?);
                complete_coupled_support_uv(
                    ctx,
                    &mut ir,
                    &pending,
                    &support,
                    &geometry,
                    (&mut coupled_attempts, &mut coupled_storage),
                    &mut sink,
                )
            } else {
                record_failed_support_uv_attempt(
                    ctx,
                    &mut direct,
                    &mut direct_storage,
                    (pending[0].0.as_str(), 0),
                    Some(&source),
                )?;
                complete_support_uv_wave(
                    ctx,
                    &mut ir,
                    SupportUvAttempts {
                        pending: &pending,
                        failed_attempts: &mut direct,
                        failed_storage: &mut direct_storage,
                        failed_coupled_attempts: &mut coupled_attempts,
                        failed_coupled_storage: &mut coupled_storage,
                        endpoint_witnesses: &mut sink,
                    },
                    &support,
                    &geometry,
                    &empty_support,
                    &geometry,
                )
            }
        });
    }
}

fn run_uncertified_chart(
    ctx: &DecodeContext<'_>,
    source: &CadIr,
    pending: &[PendingExt11SupportUv],
) -> Result<(), CodecError> {
    let mut ir = source.clone();
    let support = ctx.work_budget(1000);
    let empty_support = WorkBudget::new(0);
    let geometry = GeometryWorkBudget::from_context(ctx, 100_000);
    let mut witness_storage = ctx.reserve_scoped(0, "nx test witnesses")?;
    let mut witnesses = BTreeMap::new();
    let mut direct_storage = ctx.reserve_scoped(0, "nx test direct retry table")?;
    let mut direct = FailedSupportUvAttempts::new();
    let mut coupled_storage = ctx.reserve_scoped(0, "nx test coupled retry table")?;
    let mut coupled = FailedCoupledSupportUvAttempts::new();
    complete_support_uv_wave(
        ctx,
        &mut ir,
        SupportUvAttempts {
            pending,
            failed_attempts: &mut direct,
            failed_storage: &mut direct_storage,
            failed_coupled_attempts: &mut coupled,
            failed_coupled_storage: &mut coupled_storage,
            endpoint_witnesses: &mut EndpointWitnessSink::new(&mut witnesses, &mut witness_storage),
        },
        &support,
        &geometry,
        &empty_support,
        &geometry,
    )?;
    assert!(ir.model.procedural_curves[0]
        .intersection_context_mut()
        .unwrap()
        .sides()[0]
        .pcurve
        .is_none());
    assert_eq!(ctx.resource_refusal(), None);
    Ok(())
}

#[test]
fn chart_reproduction_refuses_each_visit_without_admitting_the_suffix() {
    let (mut ir, mut pending) = charted_fixture();
    ir.model.procedural_curves[0]
        .intersection_context_mut()
        .unwrap()
        .set_unmapped_pcurve(0, None);
    let samples = |rejected_index| {
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            (0..32)
                .map(|i| {
                    Point3::new(
                        cadmpeg_core::convert::f64_from_index(i).unwrap(),
                        0.0,
                        if i == rejected_index { 1.0 } else { 0.0 },
                    )
                })
                .collect(),
            (0..32)
                .map(|i| cadmpeg_core::convert::f64_from_index(i).unwrap())
                .collect(),
        )
        .unwrap()
    };
    pending[0].1 = samples(0);
    let error = resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx support UV chart reproduction",
        |ctx| run_uncertified_chart(ctx, &ir, &pending),
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("chart visit refusal")
    };
    assert_eq!(limit.additional, 1);
    with_decode_context(|ctx| run_uncertified_chart(ctx, &ir, &pending).unwrap());
    let work = |rejected_index| {
        let mut pending = pending.clone();
        pending[0].1 = samples(rejected_index);
        with_decode_context(|ctx| {
            run_uncertified_chart(ctx, &ir, &pending).unwrap();
            let CodecError::ResourceLimit(usage) = ctx
                .charge_work(u64::MAX, "nx test work census")
                .unwrap_err()
            else {
                panic!("work census")
            };
            usage.used
        })
    };
    // Plane evaluation is fixed work. Moving rejection from sample zero to
    // sample 31 adds exactly 31 chart visits; parameter fitting is unchanged.
    assert_eq!(work(31) - work(0), 31);
}

#[test]
fn equivalent_surface_route_charges_equal_nurbs_geometry_after_lookups() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let (mut ir, ids) = plane_model();
    for rational in [false, true] {
        let geometry = with_decode_context(|ctx| {
            let mut knots = vec![0.0];
            knots.extend((0..64).map(|i| cadmpeg_core::convert::f64_from_index(i).unwrap()));
            knots.push(63.0);
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                NurbsSurface::from_lanes(
                    ctx,
                    NurbsSurfaceAxis::new(1, knots, false),
                    NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                    NurbsSurfaceLanes::new(
                        vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 64],
                        rational.then(|| vec![vec![1.0; 2]; 64]),
                    ),
                    false,
                )
                .unwrap()
                .unwrap(),
            ))
        });
        for surface in &mut ir.model.surfaces {
            surface.geometry = geometry.clone();
        }
        let index =
            cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
        resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX equivalent surface geometry comparison",
            |ctx| {
                Ok(parameterization_equivalent_surfaces_with_index(
                    &index, &ids[0], &ids[1], ctx,
                )?)
            },
        );
        with_decode_context(|ctx| {
            assert!(
                parameterization_equivalent_surfaces_with_index(&index, &ids[0], &ids[1], ctx)
                    .unwrap()
            );
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn duplicate_attachment_candidates_charge_equal_nurbs_pcurves() {
    let (mut ir, _) = charted_fixture();
    let pcurve = long_pcurve(64);
    let context = ir.model.procedural_curves[0]
        .intersection_context_mut()
        .unwrap();
    let surface = context.sides()[0].surface.clone();
    context.set_surface(1, surface);
    for side in 0..2 {
        context.set_unmapped_pcurve(side, Some(pcurve.clone()));
    }
    for coedge in &mut ir.model.coedges {
        coedge.pcurves.clear();
    }
    assert!(!ir.model.coedges.is_empty());
    let graph = with_decode_context(|ctx| Graph::parse(ctx, &[])).unwrap();
    let run = |ctx: &DecodeContext<'_>| {
        let mut ir = ir.clone();
        let geometry = GeometryWorkBudget::from_context(ctx, 100_000);
        let stream = StreamHandle::new(ctx, cadmpeg_ir::stream_name!("nx:test"), "nx test stream")?;
        let sources = [0, 1].map(|index| IntersectionCompletionSource {
            scope: crate::decode::ids::IdScope::stream(index),
            graph: &graph,
            source_stream: stream.clone(),
            coedge_start: 0,
            procedural_start: 0,
        });
        attach_completed_intersection_pcurves_for_sources_with_budget(
            ctx,
            &mut ir,
            &sources,
            &mut AnnotationBuilder::new(),
            &BTreeMap::new(),
            &geometry,
        )?;
        assert_eq!(ctx.resource_refusal(), None);
        Ok::<_, CodecError>(())
    };
    resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx completion candidate pcurve comparison",
        run,
    );
    with_decode_context(|ctx| run(ctx).unwrap());
}
