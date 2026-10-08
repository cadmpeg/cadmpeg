// SPDX-License-Identifier: Apache-2.0
//! B5 budget and indexing regressions.

#[test]
fn b5_topology_visitor_admits_only_the_next_candidate() {
    let triangle = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut bytes = Vec::new();
    for _ in 0..4 {
        bytes.extend_from_slice(&triangle);
        bytes.extend([0; 16]);
    }
    let refusal =
        crate::test_support::with_work_refusal("catia_b5_topology_candidate_scan", |ctx| {
            super::super::visit_topology_runs(
                ctx,
                &bytes,
                &mut crate::nurbs::LaneRefusals::new(),
                |_, _| Ok(false),
            )
        });
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = refusal else {
        panic!("work refusal")
    };
    assert_eq!(limit.additional, 1);
    let mut visited = 0;
    crate::test_support::with_service_context(|ctx| {
        super::super::visit_topology_runs(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
            |_, graph| {
                assert!(!graph.faces.is_empty());
                visited += 1;
                Ok(false)
            },
        )
    })
    .expect("closed candidate graph");
    assert_eq!(visited, 1);
}

#[test]
fn class21_non_finite_first_knot_does_not_admit_the_scalar_suffix() {
    let mut payload = super::walk::a8_class21_large_test_payload(2048);
    payload[10..18].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::test_support::with_work_limit(1, |ctx| {
        super::super::parse_a8_class21_pcurve(ctx, 7, &payload)
    })
    .expect("one scalar visit rejects the lane")
    .is_none());
}

#[test]
fn b5_pcurve_evaluation_keeps_lanes_in_scratch() {
    let pcurve = super::test_pcurve(1, 2);
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..4 {
            assert_eq!(
                super::super::evaluate_pcurve(ctx, &pcurve, 0.5)
                    .expect("evaluation scratch")
                    .expect("linear pcurve"),
                [0.5, 0.0]
            );
        }
    });
}

#[test]
fn b5_expanded_pcurve_knots_pin_visit_and_emission_work() {
    let pcurve = super::test_pcurve(1, 2);
    // Three count steps include exhaustion, two pairs emit four knot values.
    assert_eq!(
        crate::test_support::with_work_limit(9, |ctx| { super::super::pcurve_knots(ctx, &pcurve) })
            .expect("nine visits and emissions")
            .expect("valid knots"),
        crate::test_support::test_b5::finite_lane(&[0.0, 0.0, 1.0, 1.0])
    );
    assert!(matches!(
        crate::test_support::with_work_limit(8, |ctx| { super::super::pcurve_knots(ctx, &pcurve) }),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}
