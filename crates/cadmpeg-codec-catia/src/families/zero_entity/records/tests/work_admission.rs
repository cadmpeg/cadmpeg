// SPDX-License-Identifier: Apache-2.0
//! Work admission for zero-entity record bindings.

#[test]
fn zero_face_loop_binding_rows_preserve_work_refusal() {
    const OPERATION: &str = "catia_zero_face_loop_binding_rows";
    let stream = crate::test_support::test_zero_entity::zero_entity_face_loop_support_stream();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::zero_entity_support_runs_in_range(
            ctx,
            &stream,
            0..stream.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    let runs = crate::test_support::with_service_context(run).expect("service budget");
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0]
            .face
            .as_ref()
            .expect("face")
            .loops
            .as_ref()
            .expect("loop roster")
            .len(),
        1
    );
    let result = crate::test_support::with_work_refusal(OPERATION, |ctx| {
        let result = run(ctx);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == OPERATION)
    );
}

#[test]
fn zero_support_slot_range_preserves_work_refusal() {
    const OPERATION: &str = "catia_zero_binding_slot_lookup";
    let mut stream = crate::test_support::test_zero_entity::zero_entity_face_loop_support_stream();
    let support_slot = 0x6a + 12 + 13;
    stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::zero_entity_support_runs_in_range(
            ctx,
            &stream,
            0..stream.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    let runs = crate::test_support::with_service_context(run).expect("service budget");
    let loops = runs[0]
        .face
        .as_ref()
        .expect("face")
        .loops
        .as_ref()
        .expect("loop roster");
    assert_eq!(loops[0].support_record_ordinals, [2]);
    let result = crate::test_support::with_work_refusal(OPERATION, |ctx| {
        let result = run(ctx);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == OPERATION)
    );
}

#[test]
fn zero_member_ranges_preserve_maximum_bounded_count() {
    let count = std::num::NonZeroUsize::new(cadmpeg_core::decode::index_from_u32(u32::MAX))
        .expect("nonzero bounded count");
    let members = super::super::ZeroEntityLoopMembers::try_new(u32::MAX, 1, count)
        .expect("largest member count fits below terminal");
    let mut ids = members.member_ids();
    assert_eq!(ids.next(), Some(u32::MAX - 1));
    assert_eq!(ids.next_back(), Some(0));
    let mut slots = members.support_slots();
    assert_eq!(slots.next(), Some(1));
    assert_eq!(slots.next_back(), Some(u32::MAX));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn zero_member_ranges_reject_count_above_identifier_extent() {
    let count = std::num::NonZeroUsize::new(
        usize::try_from(u64::from(u32::MAX) + 1).expect("64-bit count"),
    )
    .expect("nonzero count");
    assert!(super::super::ZeroEntityLoopMembers::try_new(u32::MAX, 1, count).is_none());
}

#[test]
fn endpoint_orientation_stops_at_second_missing_pair() {
    let endpoints = [None; 64];
    let result = crate::test_support::with_work_limit(2, |ctx| {
        super::super::oriented_closed_model_endpoints(ctx, &endpoints, &[true; 64])
    });
    assert_eq!(result.expect("two visited pairs fit the work budget"), None);
}

#[test]
fn constant_coordinate_search_stops_at_first_different_pole() {
    use cadmpeg_ir::geometry::{
        pcurve::{PcurveGeometry, PcurveNurbsPoles, WeightedPole2},
        SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::scalar::NonZeroReal;
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid cone"),
    ));
    for rational in [false, true] {
        let points = (0..64)
            .map(|index| Point2::new(f64::from(index), f64::from(index)))
            .collect::<Vec<_>>();
        let poles = if rational {
            PcurveNurbsPoles::Rational {
                points: points
                    .into_iter()
                    .map(|point| WeightedPole2 {
                        point,
                        weight: NonZeroReal::ONE,
                    })
                    .collect(),
            }
        } else {
            PcurveNurbsPoles::Polynomial { points }
        };
        let pcurve = crate::test_support::with_service_context(|ctx| {
            cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
                ctx,
                1,
                (0..66).map(f64::from).collect::<Vec<_>>(),
                poles,
                false,
            )
        })
        .expect("service budget")
        .expect("valid pcurve");
        let result = crate::test_support::with_work_limit(4, |ctx| {
            super::super::zero_entity_model_curve(
                ctx,
                &surface,
                &PcurveGeometry::Nurbs { nurbs: pcurve },
                [[0.0, 0.0], [63.0, 63.0]],
                &"varying coordinates",
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        assert_eq!(
            result.expect("two visits per coordinate fit the work budget"),
            None
        );
    }
}

#[test]
fn fixed_support_retains_final_knots_and_poles() {
    let bytes = super::support_pcurve_record(0x71);
    let record = super::super::ZeroEntityRecord {
        pos: 0,
        end: bytes.len(),
        tag: [0x21, 0x71],
        ordinal: 1,
    };
    // The linear support keeps four f64 knots and two finite 2D poles.
    let kept_bytes = cadmpeg_core::decode::u64_from_index(
        4 * std::mem::size_of::<f64>() + 2 * std::mem::size_of::<cadmpeg_ir::units::FinitePoint2>(),
    );
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::zero_entity_support_pcurve(
            ctx,
            &bytes,
            record,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    assert!(crate::test_support::with_retained_limit(kept_bytes, run)
        .expect("exact final lane storage fits")
        .is_some());
    crate::test_support::with_retained_limit(kept_bytes - 1, |ctx| {
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = run(ctx) else {
            panic!("both final lanes must be retained");
        };
        assert_eq!(limit.operation, "catia_zero_support_pcurve_workspace");
        assert_eq!(limit.additional, kept_bytes);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn nurbs_surface_retains_both_final_knot_lanes() {
    let bytes = super::nurbs_carrier(
        [0x34, 0xc8],
        &[0.0, 1.0, 2.0, 3.0, 4.0],
        &[4, 1, 1, 1, 4],
        &[0.0, 1.0, 2.0, 3.0, 4.0],
        &[4, 1, 1, 1, 4],
    );
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let result = super::super::zero_entity_nurbs_surface(
            ctx,
            &bytes,
            0,
            &mut crate::nurbs::LaneRefusals::new(),
        );
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("valid surface fits the service budget")
        .is_some());
    let result =
        crate::test_support::with_retained_refusal(&[], "catia_zero_nurbs_kept_knots", run);
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("both knot lanes must reach retained admission");
    };
    // Each axis has seven poles and degree three, hence eleven knots.
    assert_eq!(
        limit.additional,
        cadmpeg_core::decode::u64_from_index(2 * 11 * std::mem::size_of::<f64>()),
    );
}
