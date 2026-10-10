// SPDX-License-Identifier: Apache-2.0
//! Surface and jet storage admission.

#[test]
fn rejected_a5_scalar_and_weight_candidates_release_storage() {
    let scalar = f64::NAN.to_le_bytes();
    let mut weights = vec![0x00];
    weights.extend_from_slice(&scalar);
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(
                super::super::a5_distinct_values(ctx, &scalar, &mut 0, 1, scalar.len())
                    .expect("rejected scalar scratch")
                    .is_none()
            );
            assert!(
                super::super::a5_weights(ctx, &weights, &mut 0, 1, 1, weights.len())
                    .expect("rejected weight scratch")
                    .is_none()
            );
        }
    });
}

#[test]
fn rejected_rolling_ball_stations_release_storage() {
    let bytes = crate::test_support::test_a5a8::a8_freeform_curve_stream();
    let mut curve = crate::test_support::with_service_context(|ctx| {
        super::super::a8_freeform_curves(ctx, &bytes)
    })
    .expect("service parse")
    .pop()
    .expect("one jet");
    curve.sites[1].knot = curve.sites[0].knot;
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(super::super::rolling_ball_jet_definition(ctx, &curve)
                .expect("rejected station scratch")
                .is_none());
        }
    });
}

#[test]
fn rolling_ball_limit_retains_only_final_knots_and_poles() {
    let bytes = crate::test_support::test_a5a8::a5_freeform_curve_stream();
    let curve = crate::test_support::with_service_context(|ctx| {
        super::super::a5_freeform_curves(ctx, &bytes)
    })
    .expect("jet")
    .pop()
    .expect("one jet");
    // Twelve f64 knots and six finite XYZ poles occupy 240 bytes.
    let result = crate::test_support::with_retained_limit(12 * 8 + 6 * 24, |ctx| {
        super::super::rolling_ball_limit_curve(
            ctx,
            &curve,
            false,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("final lanes fit")
    .expect("valid curve");
    assert_eq!(result.knots().len(), 12);
    assert_eq!(result.pole_count(), 6);
}

#[test]
fn rejected_a5_surface_releases_expanded_knots() {
    let mut bytes = crate::test_support::test_a5a8::a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let record = records.first().expect("surface record");
    let frame = crate::wire::records::ConsolidatedFrame {
        pos: record.byte_offset(),
        payload: record.payload().expect("payload").start,
        end: record.range().expect("range").end,
        header_token: record.header_token(),
    };
    // Both compact knot lanes are complete. The mode after them is invalid.
    bytes[frame.payload + 2 * (1 + 1 + 1 + 2 * 8)] = 0xff;
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(super::super::a5_surface(
                ctx,
                &bytes,
                frame,
                &mut crate::nurbs::LaneRefusals::new()
            )
            .expect("rejected candidate")
            .is_none());
        }
    });
}

#[test]
fn a5_surface_retains_expanded_lanes_without_compact_header() {
    let bytes = crate::test_support::test_a5a8::a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let record = &records[0];
    let frame = crate::wire::records::ConsolidatedFrame {
        pos: record.byte_offset(),
        payload: record.payload().expect("payload").start,
        end: record.range().expect("range").end,
        header_token: record.header_token(),
    };
    // Eight f64 knots, two row headers and four XYZ poles occupy 208 bytes.
    let surface = crate::test_support::with_retained_limit(8 * 8 + 2 * 24 + 4 * 24, |ctx| {
        super::super::a5_surface(ctx, &bytes, frame, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("only final lanes retained")
    .expect("surface");
    assert_eq!(surface.geometry.u_knots().len(), 4);
    assert_eq!(surface.geometry.v_knots().len(), 4);
    assert_eq!(
        (surface.geometry.u_count(), surface.geometry.v_count()),
        (2, 2)
    );
}

#[test]
fn two_external_grid_ranges_do_not_admit_sort_or_dedup_work() {
    let one = crate::test_support::test_a5a8::a8_elided_surface_stream();
    let mut second = one.clone();
    second[7..11].copy_from_slice(&101_u32.to_le_bytes());
    // The 58-byte support pcurve precedes nine XYZ poles and a 10-byte frame.
    let pcurve_start = second.len() - 10 - 9 * 24 - 58 - 8;
    second[pcurve_start + 10..pcurve_start + 12].copy_from_slice(&101_u16.to_le_bytes());
    let bytes = [one.as_slice(), second.as_slice()].concat();
    for operation in [
        "catia_a8_external_grid_ranges_sort",
        "catia_a8_external_grid_ranges_dedup",
    ] {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let ranges = crate::test_support::with_service_context(|ctx| {
            super::super::a8_external_grid_ranges(ctx, &bytes)
        })
        .expect("a pair uses constant work");
        assert_eq!(ranges.len(), 2);
        assert!(ranges[0].start < ranges[1].start);
    }
}

#[test]
fn a8_surface_retains_expanded_lanes_without_compact_header() {
    let bytes = crate::test_support::test_a5a8::a8_surface_stream();
    // Twelve f64 knots, three row headers and nine XYZ poles occupy 384 bytes.
    let surface = crate::test_support::with_retained_limit(12 * 8 + 3 * 24 + 9 * 24, |ctx| {
        super::super::resolved_a8_surface_from_object_frame(
            ctx,
            &bytes,
            0,
            bytes.len(),
            0xdeca_fbad,
            &mut super::super::A8ExternalGridSites::default(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("only final lanes retained")
    .expect("surface");
    assert_eq!(surface.geometry.u_knots().len(), 6);
    assert_eq!(surface.geometry.v_knots().len(), 6);
    assert_eq!(
        (surface.geometry.u_count(), surface.geometry.v_count()),
        (3, 3)
    );
}

#[test]
fn b5_object_pcurve_admits_source_knots_controls_and_multiplicities() {
    let bytes = crate::test_support::test_a5a8::a8_pcurve_stream();
    let records = [crate::families::b5::graph::B5Record {
        offset: 0,
        family: 0xa8,
        class: 0x20,
        object_id: 0x5678,
        payload: &bytes[11..],
    }];
    // Two source knots (16 bytes) and six UV controls (96 bytes) precede
    // the two u32 multiplicities (8 bytes) in the production graph route.
    let result = crate::test_support::with_retained_limit(16 + 96 + 8 - 1, |ctx| {
        crate::families::b5::graph::parse_from_records(
            ctx,
            &bytes,
            &records,
            &[],
            false,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_B5_object_pcurve_multiplicities"
            && limit.used == 112 && limit.additional == 8)
    );
}
