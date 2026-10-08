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

#[test]
fn b5_point_index_rejects_first_coordinate_before_the_suffix() {
    let mut points = vec![crate::test_support::test_b5::point([f64::MAX, 0.0, 0.0])];
    points.extend(std::iter::repeat_n(
        crate::test_support::test_b5::point([0.0; 3]),
        10_000,
    ));
    assert!(matches!(
        crate::test_support::with_work_limit(1, |ctx| { super::super::point_index(ctx, &points) }),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn b5_analytic_pcurve_range_drops_its_geometry_lanes() {
    let mut payload = vec![0x81];
    payload.extend(crate::test_support::test_b5::b5_object_ref(1));
    payload.push(0x05);
    for value in [0.0_f64, 0.0, 1.0] {
        payload.extend(value.to_le_bytes());
    }
    let record = super::super::B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x18,
        object_id: 2,
        payload: &payload,
    };
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..4 {
            assert_eq!(
                super::super::analytic_pcurve_range(ctx, &record).expect("scoped range geometry"),
                Some(crate::test_support::test_b5::finite_pair([0.0, 1.0]))
            );
        }
    });
}

#[test]
fn b5_rejected_face_loop_and_class21_output_releases_candidate_storage() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut owned = super::super::records(&bytes)
        .into_iter()
        .find(|record| record.class == 0x62)
        .expect("triangle loop")
        .payload
        .to_vec();
    owned.pop();
    let loop_record = super::super::B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x62,
        object_id: 1,
        payload: &owned,
    };
    let face_record = super::super::B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x5f,
        object_id: 2,
        payload: &[0x82, 0x81, 0x82],
    };
    let mut pcurve = super::walk::a8_class21_large_test_payload(2048);
    pcurve[10..18].copy_from_slice(&f64::NAN.to_le_bytes());
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..4 {
            assert!(super::super::parse_loop_record(ctx, &loop_record)
                .expect("rejected loop scratch")
                .is_none());
            assert!(super::super::parse_face_record(ctx, &face_record)
                .expect("rejected face scratch")
                .is_none());
            assert!(super::super::parse_a8_class21_pcurve(ctx, 7, &pcurve)
                .expect("rejected pcurve scratch")
                .is_none());
        }
    });
}

#[test]
fn b5_frame_iterator_admits_only_the_requested_frame_prefix() {
    let mut bytes = Vec::new();
    crate::test_support::test_b5::append_b5_record(&mut bytes, 0x27, 9, &[0x80]);
    bytes.extend(std::iter::repeat_n(0, 10_000));
    crate::test_support::with_work_limit(1, |ctx| {
        let mut frames = super::super::object_stream_frames(ctx, &bytes)?;
        assert_eq!(frames.next().expect("first frame")?.object_id, 9);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("one source step returns the first frame");
}
