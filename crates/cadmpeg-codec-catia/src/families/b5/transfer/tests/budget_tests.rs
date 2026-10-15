// SPDX-License-Identifier: Apache-2.0
//! Cross-pass bookkeeping ownership.

#[test]
fn b5_transfer_bookkeeping_uses_a_live_materialized_owner() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph construction budget")
    .expect("closed topology");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#budget-plan".to_owned())
        .expect("identity grammar");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        crate::test_support::with_materialized_limit(0, |ctx| {
            let mut curve_storage = std::collections::BTreeMap::new();
            let mut storage = ctx.reserve_scoped(0, "test_plan_storage")?;
            super::super::build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
                (&mut storage, &mut curve_storage),
            )
        })
    else {
        panic!("plan bookkeeping refusal")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes
    );
    assert_eq!(limit.operation, "catia b5 face ownership ids");
}

#[test]
fn b5_transfer_plan_preserves_finite_knot_admission() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph admission")
    .expect("closed topology");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#finite-plan".to_owned())
        .expect("identity grammar");
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "IR NURBS knot finiteness",
            None,
        );
        let mut curve_storage = std::collections::BTreeMap::new();
        let mut storage = ctx
            .reserve_scoped(0, "test_plan_storage")
            .expect("scratch owner");
        let plan = super::super::build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
            (&mut storage, &mut curve_storage),
        )
        .expect("finite knots need no repeated finiteness scan")
        .expect("closed transfer plan");
        assert_eq!(plan.pcurve_plan.len(), 3);
        for (geometry, _, _) in plan.pcurve_plan.values() {
            let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
                panic!("native pcurve must keep its NURBS carrier")
            };
            assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        }
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn b5_extrusion_support_preserves_finite_knot_admission() {
    use crate::families::b5::graph::{B5ExtrusionDirectrix, B5ExtrusionSurface};
    use crate::test_support::test_b5::{finite_pair, increasing, increasing_bounds, unit};

    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph admission")
    .expect("closed topology");
    graph.extrusion_surfaces.insert(
        900,
        B5ExtrusionSurface {
            object_id: 900,
            direction: unit([0.0, 0.0, 1.0]),
            parameter_bounds: increasing_bounds([[-1.0, 1.0], [0.0, 1.0]]),
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 901,
                support: (100, 200, finite_pair([0.0, 1.0])),
                parameter_range: increasing([0.0, 1.0]),
            },
        },
    );
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "IR NURBS knot finiteness",
            None,
        );
        let extrusion = super::super::resolved_extrusion_surface(
            ctx,
            &graph,
            900,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("finite knots need no repeated finiteness scan")
        .expect("exact extrusion support");
        let super::super::ResolvedExtrusionDirectrix::SurfaceCurve { support, .. } =
            extrusion.directrix
        else {
            panic!("one-support extrusion directrix")
        };
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = support.pcurve else {
            panic!("native support pcurve must keep its NURBS carrier")
        };
        assert_eq!(support.surface_object_id, 100);
        assert_eq!(support.pcurve_parameter_range, [0.0, 1.0]);
        assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn b5_selected_curve_candidates_stay_scoped_until_emission() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph budget")
    .expect("closed triangle");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#scoped-curves".to_owned())
        .expect("identity");
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..3 {
            let mut storage = ctx
                .reserve_scoped(0, "test_plan_storage")
                .expect("plan owner");
            let mut curves = std::collections::BTreeMap::new();
            let plan = super::super::build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
                (&mut storage, &mut curves),
            )
            .expect("all plan geometry is temporary")
            .expect("closed plan");
            assert_eq!(plan.pcurve_plan.len(), 3);
            assert_eq!(plan.edge_curve_plan.len(), 3);
            assert_eq!(curves.len(), 3);
        }
    });
}

#[test]
fn b5_emitter_identity_copies_use_materialized_bookkeeping_storage() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph admission")
    .expect("closed graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#emitter-storage".to_owned())
        .expect("identity");
    for operation in ["catia_b5_pcurve_use_id", "catia_b5_edge_map_id"] {
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut plan = crate::test_support::with_service_context(|plan_ctx| {
                super::build_plan(
                    plan_ctx,
                    &graph,
                    &payload,
                    &mut crate::nurbs::LaneRefusals::new(),
                )
            })
            .expect("plan admission")
            .expect("closed plan");
            plan.exact_support_edges.clear();
            plan.exact_support_curves.clear();
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            let mut storage = ctx.reserve_scoped(0, "test_emitter_storage")?;
            let mut ir = cadmpeg_ir::CadIr::empty();
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            if operation == "catia_b5_pcurve_use_id" {
                let uses = super::super::pcurves::emit_pcurves(
                    &mut ir,
                    &mut annotations,
                    &graph,
                    &plan,
                    &mut admission,
                    &mut storage,
                )?;
                assert_eq!(uses.len(), 3);
                assert_eq!(ir.model.pcurves.len(), 3);
            } else {
                let edges = super::super::edges::emit_edges(
                    &mut ir,
                    &mut annotations,
                    &graph,
                    &payload,
                    &mut plan,
                    &std::collections::BTreeMap::new(),
                    (
                        &mut admission,
                        &mut storage,
                        &mut std::collections::BTreeMap::new(),
                    ),
                )?;
                assert_eq!(edges.len(), 3);
                assert_eq!(ir.model.edges.len(), 3);
            }
            Ok::<_, cadmpeg_core::CodecError>(())
        };
        let refused = crate::test_support::with_work_refusal(operation, run);
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation)
        );
        // The work probe proves this copy runs. Its storage must bypass retained
        // admission even when an earlier temporary allocation has a higher peak.
        crate::test_support::with_retained_limit(u64::MAX, |ctx| {
            let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
            run(ctx)
        })
        .expect("lookup identities use materialized ownership");
    }
}

#[test]
fn b5_rejected_owned_orientation_restores_the_exact_carrier() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let original = crate::test_support::with_service_context(|ctx| {
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            ctx,
            1,
            vec![0.0, 0.0, 10.0, 10.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
            ],
            Some(vec![1.0, 2.0]),
            false,
        )
    })
    .expect("curve admission")
    .expect("rational curve");
    let original = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(original));
    for parameters in [[2.0, 8.0], [8.0, 2.0]] {
        let mut geometry = original.clone();
        let fitted = crate::test_support::with_retained_limit(0, |ctx| {
            super::super::pcurves::oriented_nurbs_range(
                ctx,
                &mut geometry,
                parameters,
                [100.0; 3],
                [200.0; 3],
            )
        })
        .expect("rejected orientation retains no copy");
        assert!(fitted.is_none());
        assert_eq!(geometry, original);
    }
}

#[test]
fn b5_revolution_queries_retain_only_the_requested_lanes() {
    use crate::families::b5::graph::{B5Profile, B5Surface};
    use crate::test_support::test_b5::{increasing, point, unit};
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph budget")
    .expect("closed graph");
    graph.profiles.insert(
        110,
        B5Profile::Line {
            point: point([1.0, 0.0, 0.0]),
            direction: crate::checked::ExactUnitVector3::new([0.0, 0.0, 1.0])
                .expect("unit direction"),
            parameter_range: increasing([0.0, 1.0]),
        },
    );
    graph.surfaces.insert(
        100,
        B5Surface::Revolution {
            profile_curve: 110,
            axis_origin: point([0.0; 3]),
            axis_direction: unit([0.0, 0.0, 1.0]),
            profile_range: increasing([0.0, 1.0]),
            angular_range: increasing([0.0, std::f64::consts::FRAC_PI_2]),
            angular_scale: cadmpeg_ir::scalar::PositiveReal::new(1.0).expect("positive scale"),
        },
    );
    let payload =
        cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#revolution-plan").expect("identity");
    crate::test_support::with_retained_limit(0, |ctx| {
        let mut storage = ctx
            .reserve_scoped(0, "test_revolution_plan_storage")
            .expect("plan owner");
        let mut curve_storage = std::collections::BTreeMap::new();
        let plan = super::super::build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
            (&mut storage, &mut curve_storage),
        )
        .expect("surface plan geometry remains materialized")
        .expect("revolution plan");
        assert_eq!(plan.surface_plan.len(), 1);
        assert!(matches!(
            plan.surface_plan[&100].procedure,
            Some(super::super::SurfaceProcedure::Revolution(_))
        ));
        assert!(matches!(
            plan.surface_plan[&100].geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(_)
            )
        ));
    });
    // The surface owns 10 knots, two row descriptors and six weighted poles.
    let geometry = crate::test_support::with_retained_limit(10 * 8 + 2 * 24 + 6 * 32, |ctx| {
        super::super::resolved_surface_geometry(
            ctx,
            &graph,
            100,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("discarded procedure is materialized")
    .expect("revolution geometry");
    assert!(matches!(
        geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(_)
        )
    ));
    // The construction owns four knots and two polynomial poles; its cache is temporary.
    let construction = crate::test_support::with_retained_limit(4 * 8 + 2 * 24, |ctx| {
        super::super::resolved_revolution_surface(
            ctx,
            &graph,
            100,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("discarded surface cache is materialized")
    .expect("revolution construction");
    assert_eq!(
        construction.directrix.knots().as_slice(),
        &[0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        construction.angular_interval,
        [0.0, std::f64::consts::FRAC_PI_2]
    );
}

#[test]
fn b5_object_stream_jet_inputs_leave_only_the_pcurve_retained() {
    let bytes = crate::test_support::test_a5a8::a8_pcurve_stream();
    let pcurves = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::object_stream_pcurves(ctx, &bytes)
    })
    .expect("jet fixture budget");
    assert_eq!(pcurves.len(), 1);
    let graph_bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &graph_bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph budget")
    .expect("closed graph");
    let surface = graph.surfaces.get(&100).expect("plane carrier");
    // A degree-five two-site jet owns 12 knots and six finite two-coordinate poles.
    let resolved = crate::test_support::with_retained_limit(12 * 8 + 6 * 16, |ctx| {
        super::super::resolved_object_stream_pcurve(
            ctx,
            &pcurves[0],
            surface,
            None,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("raw jet inputs are materialized")
    .expect("resolved jet");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = resolved.geometry else {
        panic!("jet NURBS carrier")
    };
    assert_eq!(
        nurbs.knots().as_slice(),
        &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(resolved.parameter_range, [0.0, 1.0]);
}

#[test]
fn b5_face_emission_bookkeeping_uses_materialized_storage() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph budget")
    .expect("closed graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#face-storage".to_owned())
        .expect("identity");
    for operation in [
        "catia_b5_face_component_members",
        "catia_b5_region_ids",
        "catia_b5_face_loop_ids",
        "catia_b5_coedge_ids_by_member",
        "catia_b5_coedge_emit_id",
    ] {
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::super::transfer_complete(
                &mut cadmpeg_ir::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
                &mut crate::families::FamilyEntityAdmission::new(ctx),
            )
        };
        let dimension = if operation == "catia_b5_coedge_emit_id" {
            ResourceDimension::WorkUnits
        } else {
            ResourceDimension::CollectionItems
        };
        let refused =
            cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                if dimension == ResourceDimension::WorkUnits {
                    crate::test_support::with_work_limit(cap, run)
                } else {
                    crate::test_support::with_collection_limit(cap, run)
                }
            });
        assert!(
            matches!(refused, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == operation)
        );
        // Collection or copy admission proves the operation runs. Retained
        // admission must not see its temporary backing.
        assert!(crate::test_support::with_retained_limit(u64::MAX, |ctx| {
            let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
            run(ctx)
        })
        .expect("face bookkeeping uses materialized ownership"));
    }
    let model = crate::test_support::with_service_context(|ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        assert!(super::super::transfer_complete(
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut crate::families::FamilyEntityAdmission::new(ctx)
        )?);
        Ok::<_, cadmpeg_core::CodecError>(ir.model)
    })
    .expect("complete model admission");
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.loops.len(), 1);
    assert_eq!(model.coedges.len(), 3);
    for coedge in &model.coedges {
        assert_eq!(coedge.radial_next.as_str(), coedge.id.as_str());
    }
}

#[test]
fn b5_rejected_surface_candidates_release_partial_geometry() {
    use crate::families::b5::graph::{B5ExtrusionDirectrix, B5ExtrusionSurface, B5Profile};
    use crate::test_support::test_b5::{finite_pair, increasing, increasing_bounds, point, unit};
    let profile = B5Profile::Line {
        point: point([1.0, 0.0, 0.0]),
        direction: crate::checked::ExactUnitVector3::new([0.0, 0.0, 1.0]).expect("unit direction"),
        parameter_range: increasing([0.0, 1.0]),
    };
    // The profile is valid. A zero angular span rejects its surface after profile construction.
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..3 {
            let mut refusal = crate::nurbs::LaneRefusals::new();
            assert!(super::super::surfaces::revolution_surface(
                ctx,
                Some(&profile),
                (point([0.0; 3]), unit([0.0, 0.0, 1.0])),
                cadmpeg_ir::scalar::PositiveReal::new(1.0).expect("positive scale"),
                [[0.0, 1.0], [0.0, 0.0]],
                &"rejected revolution",
                &mut refusal,
            )
            .expect("rejected profile lanes are temporary")
            .is_none());
            assert!(refusal.take_notes().is_empty());
        }
    });
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph budget")
    .expect("closed graph");
    // Both native support lanes resolve; equal surface identities reject the pair.
    graph.extrusion_surfaces.insert(
        900,
        B5ExtrusionSurface {
            object_id: 900,
            direction: unit([0.0, 0.0, 1.0]),
            parameter_bounds: increasing_bounds([[-1.0, 1.0], [0.0, 1.0]]),
            directrix: B5ExtrusionDirectrix::Intersection {
                object_id: 901,
                supports: [(100, 200, finite_pair([0.0, 1.0])); 2],
                parameter_range: increasing([0.0, 1.0]),
                cache_fit_tolerance: cadmpeg_ir::scalar::PositiveReal::new(0.01)
                    .expect("positive tolerance"),
            },
        },
    );
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..3 {
            let mut refusal = crate::nurbs::LaneRefusals::new();
            assert!(
                super::super::resolved_extrusion_surface(ctx, &graph, 900, &mut refusal)
                    .expect("rejected support lanes are temporary")
                    .is_none()
            );
            assert!(refusal.take_notes().is_empty());
        }
    });
}
