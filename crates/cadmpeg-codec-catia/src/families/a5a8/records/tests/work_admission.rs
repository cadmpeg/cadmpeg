use super::assert_a5_surface_collection_refusal;
use crate::test_support::test_a5a8::{a5_surface_short_tail, a5_surface_stream, a5_surface_tail};

use super::super::{a5_knots, a5_nurbs_curves};
use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn surface_tail_continuation_refusal_propagates_unchanged() {
    let bytes = crate::test_support::test_a5a8::a5_surface_tail();
    crate::test_support::with_work_limit(0, |ctx| {
        let result = super::super::parse_surface_tail(ctx, &bytes, 0, bytes.len());
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("surface-tail continuation work refusal required")
        };
        assert_eq!(limit.operation, "catia_a5_surface_tail_continuation_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

fn work_refusals<T>(
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> std::collections::HashSet<&'static str> {
    let mut operations = std::collections::HashSet::new();
    let mut cap = 0;
    for _ in 0..1024 {
        match crate::test_support::with_work_limit(cap, &run) {
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits =>
            {
                operations.insert(limit.operation);
                cap = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("finite fixture work");
            }
            Ok(_) => return operations,
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    panic!("fixture did not finish its admitted work");
}

#[test]
fn a5_knots_refuse_multiplicity_scan_and_expansion_work() {
    let operations = work_refusals(|ctx| a5_knots(ctx, &[0.0, 1.0], 1));
    for operation in [
        "catia_a5_multiplicity_emit",
        "catia_a5_knot_expansion_scan",
        "catia_a5_knot_expansion_emit",
    ] {
        assert!(
            operations.contains(operation),
            "missing work refusal for {operation}"
        );
    }
    assert_eq!(
        crate::test_support::with_service_context(|ctx| a5_knots(ctx, &[0.0, 1.0], 1))
            .expect("service work"),
        Some((vec![0.0, 0.0, 1.0, 1.0], 2))
    );
}

#[test]
fn a5_nurbs_knots_refuse_scan_and_expansion_work() {
    let bytes = super::curve_and_guide_records::a5_nurbs_curve_stream();
    let operations = work_refusals(|ctx| a5_nurbs_curves(ctx, &bytes));
    for operation in [
        "catia_a5_nurbs_knot_expansion_scan",
        "catia_a5_nurbs_knot_expansion_emit",
    ] {
        assert!(
            operations.contains(operation),
            "missing work refusal for {operation}"
        );
    }
    assert_eq!(
        crate::test_support::with_service_context(|ctx| a5_nurbs_curves(ctx, &bytes))
            .expect("service work")
            .len(),
        1
    );
}

#[test]
fn a8_frame_scanning_refuses_work_before_empty_result() {
    let bytes = [0; 32];
    let result = crate::test_support::with_work_limit(0, |ctx| {
        super::super::a8_freeform_curves(ctx, &bytes)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_a8_frame_scan"));
    let result = crate::test_support::with_work_limit(0, |ctx| {
        super::super::object_stream_pcurves(ctx, &bytes)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_object_stream_frame_scan"));
}

fn require_work_operations<T>(
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
    required: &[&str],
) {
    let operations = work_refusals(run);
    for operation in required {
        assert!(
            operations.contains(operation),
            "missing work refusal for {operation}"
        );
    }
}

#[test]
fn a5_nurbs_preflight_and_materialization_refuse_caller_work() {
    let bytes = super::curve_and_guide_records::a5_nurbs_curve_stream();
    require_work_operations(
        |ctx| a5_nurbs_curves(ctx, &bytes),
        &[
            "catia_a5_nurbs_record_scan",
            "catia_a5_nurbs_preflight",
            "catia_a5_nurbs_knot_preflight_scan",
            "catia_a5_nurbs_control_preflight_scan",
            "catia_a5_nurbs_knot_materialization",
            "catia_a5_nurbs_pole_materialization",
        ],
    );
}

#[test]
fn a5_guide_preflight_and_materialization_refuse_caller_work() {
    let bytes = crate::test_support::test_a5a8::a5_guide_curve_stream();
    require_work_operations(
        |ctx| super::super::a5_guide_curves(ctx, &bytes),
        &[
            "catia_a5_guide_record_scan",
            "catia_a5_guide_knot_scan",
            "catia_a5_guide_materialization",
        ],
    );
    let curve =
        crate::test_support::with_service_context(|ctx| super::super::a5_guide_curves(ctx, &bytes))
            .expect("service parse")
            .pop()
            .expect("one guide");
    require_work_operations(|ctx| curve.knots(ctx), &["catia_a5_guide_knot_projection"]);
}

#[test]
fn a5_jet_preflight_materialization_and_projection_refuse_caller_work() {
    let bytes = crate::test_support::test_a5a8::a5_freeform_curve_stream();
    require_work_operations(
        |ctx| super::super::a5_freeform_curves(ctx, &bytes),
        &[
            "catia_a5_jet_record_scan",
            "catia_a5_jet_preflight",
            "catia_a5_jet_knot_preflight_scan",
            "catia_a5_jet_materialization",
        ],
    );
    let curve = crate::test_support::with_service_context(|ctx| {
        super::super::a5_freeform_curves(ctx, &bytes)
    })
    .expect("service parse")
    .pop()
    .expect("one jet");
    require_work_operations(
        |ctx| {
            super::super::rolling_ball_limit_curve(
                ctx,
                &curve,
                false,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        },
        &[
            "catia_a5_limit_jet_projection",
            "catia_a5_jet_knot_projection",
            "catia_a5_limit_pole_projection",
        ],
    );
}

#[test]
fn a8_jet_preflight_materialization_and_projection_refuse_caller_work() {
    let bytes = crate::test_support::test_a5a8::a8_freeform_curve_stream();
    require_work_operations(
        |ctx| super::super::a8_freeform_curves(ctx, &bytes),
        &[
            "catia_a8_frame_scan",
            "catia_a8_jet_preflight",
            "catia_a8_jet_knot_preflight_scan",
            "catia_a8_jet_materialization",
        ],
    );
    let curve = crate::test_support::with_service_context(|ctx| {
        super::super::a8_freeform_curves(ctx, &bytes)
    })
    .expect("service parse")
    .pop()
    .expect("one jet");
    require_work_operations(
        |ctx| curve.multiplicities(ctx),
        &["catia_a8_jet_multiplicity_projection"],
    );
    require_work_operations(
        |ctx| super::super::rolling_ball_jet_definition(ctx, &curve),
        &["catia_a8_jet_station_projection"],
    );
}

#[test]
fn object_stream_pcurve_preflight_materialization_and_projection_refuse_caller_work() {
    let bytes = crate::test_support::test_a5a8::a8_pcurve_stream();
    require_work_operations(
        |ctx| super::super::object_stream_pcurves(ctx, &bytes),
        &[
            "catia_object_stream_frame_scan",
            "catia_object_stream_pcurve_preflight",
            "catia_object_stream_pcurve_lane_scan",
            "catia_object_stream_pcurve_materialization",
        ],
    );
    let curve = crate::test_support::with_service_context(|ctx| {
        super::super::object_stream_pcurves(ctx, &bytes)
    })
    .expect("service parse")
    .pop()
    .expect("one pcurve");
    require_work_operations(|ctx| curve.knots(ctx), &["catia_a8_pcurve_knot_projection"]);
    require_work_operations(
        |ctx| curve.bspline(ctx),
        &["catia_a8_pcurve_jet_projection"],
    );
}

#[test]
fn a8_lane_preflight_and_inline_grid_materialization_refuse_caller_work() {
    let mut bytes = crate::test_support::test_a5a8::a8_rational_surface_stream();
    bytes.extend_from_slice(&crate::test_support::test_a5a8::a8_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 11).expect("fixture payload width");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());
    require_work_operations(
        |ctx| super::super::a8_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new()),
        &[
            "catia_a8_frame_scan",
            "catia_a8_lane_preflight",
            "catia_a8_distinct_knot_preflight_scan",
            "catia_a8_distinct_materialization",
            "catia_a8_multiplicity_materialization",
            "catia_a8_pole_count_scan",
            "catia_a8_inline_pole_materialization",
            "catia_a8_inline_weight_materialization",
            "catia_a8_surface_suffix_scan",
            "catia_a8_inline_pole_rows",
            "catia_a8_inline_weight_rows",
        ],
    );
}

#[test]
fn a5_surface_knot_and_grid_materialization_refuse_caller_work() {
    let bytes = crate::test_support::test_a5a8::a5_rational_surface_stream();
    require_work_operations(
        |ctx| super::super::a5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new()),
        &[
            "catia_a5_surface_record_scan",
            "catia_a5_distinct_materialization",
            "catia_a5_surface_knot_order_scan",
            "catia_a5_surface_pole_materialization",
            "catia_a5_surface_tail_continuation_scan",
            "catia_a5_surface_pole_rows",
            "catia_a5_surface_weight_rows",
        ],
    );
}

#[test]
fn external_grid_inspection_and_materialization_preserve_work_refusals() {
    let bytes = crate::test_support::test_a5a8::a8_elided_surface_stream();
    let frame = crate::test_support::with_service_context(|ctx| {
        super::super::a8_frames(ctx, &bytes, 0x34).map(|mut frames| frames.next())
    })
    .expect("service context admits A8 frames")
    .expect("one A8 surface frame");
    let header = crate::test_support::with_service_context(|ctx| {
        super::super::parse_a8_surface_header(ctx, &bytes, frame)
    })
    .expect("service header")
    .expect("complete header")
    .header;
    require_work_operations(
        |ctx| {
            super::super::a8_surface_from_external_grid(
                ctx,
                &bytes,
                &header,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        },
        &[
            "catia_external_grid_frame_scan",
            "catia_a8_external_grid_candidate_scan",
            "catia_a8_external_pole_materialization",
            "catia_a8_external_pole_rows",
        ],
    );
    require_work_operations(
        |ctx| super::super::a8_external_grid_ranges(ctx, &bytes),
        &[
            "catia_a8_lane_preflight",
            "catia_a8_external_grid_candidate_scan",
        ],
    );
}

#[test]
fn mirrored_weight_program_refuses_seed_and_copy_work() {
    let mut bytes = vec![0x01, 0x03, 0x00];
    for weight in [1.0_f64, 0.8] {
        bytes.extend_from_slice(&weight.to_le_bytes());
    }
    bytes.push(0x02);
    require_work_operations(
        |ctx| super::super::a5_weights(ctx, &bytes, &mut 0, 2, 4, bytes.len()),
        &[
            "catia_a5_weight_row_scan",
            "catia_a5_weight_seed_scan",
            "catia_a5_weight_mirror_copy",
            "catia_a5_weight_previous_row_copy",
        ],
    );
    let weights = crate::test_support::with_service_context(|ctx| {
        super::super::a5_weights(ctx, &bytes, &mut 0, 2, 4, bytes.len())
    })
    .expect("service weights")
    .expect("complete mirrored grid");
    assert_eq!(
        weights
            .iter()
            .map(|weight| weight.get())
            .collect::<Vec<_>>(),
        [1.0, 0.8, 0.8, 1.0, 1.0, 0.8, 0.8, 1.0]
    );
}

#[test]
fn explicit_weight_program_refuses_lane_read_work() {
    let mut bytes = vec![0x00];
    for weight in [1.0_f64, 2.0, 3.0, 4.0] {
        bytes.extend_from_slice(&weight.to_le_bytes());
    }
    require_work_operations(
        |ctx| super::super::a5_weights(ctx, &bytes, &mut 0, 2, 2, bytes.len()),
        &["catia_a5_explicit_weight_materialization"],
    );
}

#[test]
fn grid_partition_refuses_move_work_before_rows_are_created() {
    let result = crate::test_support::with_work_limit(0, |ctx| {
        super::super::grid_rows(ctx, vec![0; 8], 4, "test grid partition")
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "test grid partition"));
    assert_eq!(
        crate::test_support::with_work_limit(10, |ctx| {
            super::super::grid_rows(ctx, vec![0; 8], 4, "test grid partition")
        })
        .expect("two rows and eight moves"),
        vec![vec![0; 4], vec![0; 4]]
    );
}

#[test]
fn external_rational_grid_inspection_and_weight_copy_refuse_caller_work() {
    let mut bytes = crate::test_support::test_a5a8::a8_elided_surface_stream();
    bytes[58] = 0x05;
    let next_frame = bytes.len() - 10;
    let weights = std::iter::repeat_n(2.0_f64, 9).flat_map(f64::to_le_bytes);
    bytes.splice(next_frame..next_frame, weights);
    let frame = crate::test_support::with_service_context(|ctx| {
        super::super::a8_frames(ctx, &bytes, 0x34).map(|mut frames| frames.next())
    })
    .expect("service context admits A8 frames")
    .expect("one surface frame");
    let header = crate::test_support::with_service_context(|ctx| {
        super::super::parse_a8_surface_header(ctx, &bytes, frame)
    })
    .expect("service header")
    .expect("complete header")
    .header;
    require_work_operations(
        |ctx| {
            super::super::a8_surface_from_external_grid(
                ctx,
                &bytes,
                &header,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        },
        &[
            "catia_a8_external_grid_weight_scan",
            "catia_a8_external_weight_materialization",
            "catia_a8_external_weight_rows",
        ],
    );
    let surface = crate::test_support::with_service_context(|ctx| {
        super::super::a8_surface_from_external_grid(
            ctx,
            &bytes,
            &header,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service external grid")
    .expect("complete rational surface");
    assert_eq!(
        surface
            .geometry
            .pole_grid()
            .weights()
            .expect("rational grid")
            .concat(),
        vec![2.0; 9]
    );
}

#[test]
fn surface_tail_scans_continuation_without_materializing_a_lane() {
    let mut short = a5_surface_short_tail();
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::parse_surface_tail(ctx, &short, 0, short.len())
        })
        .expect("service work"),
        Some(short.len())
    );
    short[71..79].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::parse_surface_tail(ctx, &short, 0, short.len())
        })
        .expect("service work"),
        None
    );
    let mut long = a5_surface_tail();
    long[71..79].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::parse_surface_tail(ctx, &long, 0, long.len())
        })
        .expect("service work"),
        Some(long.len())
    );
    long[71..79].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::parse_surface_tail(ctx, &long, 0, long.len())
        })
        .expect("service work"),
        None
    );
}

#[test]
fn a5_distinct_knots_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 0, "catia_a5_distinct_knots");
}

#[test]
fn a5_multiplicities_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 4, "catia_a5_knot_multiplicities");
}

#[test]
fn a5_expanded_knots_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 6, "catia_a5_expanded_knots");
}
