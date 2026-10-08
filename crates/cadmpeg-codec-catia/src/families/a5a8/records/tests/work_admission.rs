use super::assert_a5_surface_collection_refusal;
use crate::test_support::test_a5a8::{a5_surface_short_tail, a5_surface_stream, a5_surface_tail};

use super::super::{a5_knots, a5_nurbs_curves};
use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

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

fn require_sticky_work_refusal<T>(
    operation: &'static str,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            crate::test_support::with_work_limit(cap, |ctx| {
                let result = run(ctx);
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn a5_knots_refuse_knot_visit_and_expansion_work() {
    let operations = work_refusals(|ctx| a5_knots(ctx, &[0.0, 1.0], 1));
    for operation in ["catia_a5_knot_expansion_scan", "catia_a5_expanded_knots"] {
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
        "catia_a5_nurbs_expanded_knots",
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
            "catia_a5_nurbs_knot_preflight_scan",
            "catia_a5_nurbs_control_preflight_scan",
            "catia_a5_nurbs_control_points",
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
        &["catia_a5_jet_record_scan", "catia_a5_jet_materialization"],
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
            "catia_a8_jet_multiplicity_preflight_scan",
            "catia_a8_jet_materialization",
        ],
    );
    let curve = crate::test_support::with_service_context(|ctx| {
        super::super::a8_freeform_curves(ctx, &bytes)
    })
    .expect("service parse")
    .pop()
    .expect("one jet");
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curve.multiplicities(ctx))
            .expect("service multiplicities"),
        [6, 6]
    );
    require_sticky_work_refusal("catia_a8_jet_multiplicity_preflight_scan", |ctx| {
        super::super::a8_freeform_curves(ctx, &bytes)
    });
    require_work_operations(
        |ctx| curve.multiplicities(ctx),
        &["catia_a8_jet_multiplicities"],
    );
    require_work_operations(
        |ctx| super::super::rolling_ball_jet_definition(ctx, &curve),
        &["catia_a8_jet_stations"],
    );
}

#[test]
fn object_stream_pcurve_preflight_materialization_and_projection_refuse_caller_work() {
    let bytes = crate::test_support::test_a5a8::a8_pcurve_stream();
    require_work_operations(
        |ctx| super::super::object_stream_pcurves(ctx, &bytes),
        &[
            "catia_object_stream_frame_scan",
            "catia_object_stream_pcurve_multiplicity_preflight_scan",
            "catia_object_stream_pcurve_materialization",
        ],
    );
    let curve = crate::test_support::with_service_context(|ctx| {
        super::super::object_stream_pcurves(ctx, &bytes)
    })
    .expect("service parse")
    .pop()
    .expect("one pcurve");
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curve.knots(ctx))
            .expect("service pcurve knots")
            .len(),
        2
    );
    require_sticky_work_refusal(
        "catia_object_stream_pcurve_multiplicity_preflight_scan",
        |ctx| super::super::object_stream_pcurves(ctx, &bytes),
    );
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
            "catia_a8_distinct_knot_preflight_scan",
            "catia_a8_surface_multiplicity_preflight_scan",
            "catia_a8_child_frame_scan",
            "catia_a8_distinct_materialization",
            "catia_a8_multiplicity_materialization",
            "catia_a8_pole_count_scan",
            "catia_a8_inline_poles",
            "catia_a8_inline_weights",
        ],
    );
    let surfaces = crate::test_support::with_service_context(|ctx| {
        super::super::a8_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service A8 surfaces");
    assert_eq!(surfaces.len(), 1);
    assert_eq!(
        surfaces[0]
            .geometry
            .pole_grid()
            .weights()
            .map(|rows| rows.concat()),
        Some(vec![2.0; 9])
    );
    require_sticky_work_refusal("catia_a8_surface_multiplicity_preflight_scan", |ctx| {
        super::super::a8_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    });
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
            "catia_a5_knot_expansion_scan",
            "catia_a5_surface_poles",
            "catia_a5_weight_row_scan",
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
                &mut super::super::A8ExternalGridSites::default(),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        },
        &[
            "catia_object_stream_frame_scan",
            "catia_a8_external_grid_candidate_visits",
            "catia_a8_external_grid_candidate_scan",
            "catia_a8_external_poles",
        ],
    );
    require_work_operations(
        |ctx| super::super::a8_external_grid_ranges(ctx, &bytes),
        &[
            "catia_a8_distinct_knot_preflight_scan",
            "catia_a8_external_grid_candidate_visits",
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
            .concat()
            .iter()
            .map(|weight| weight.get())
            .collect::<Vec<_>>(),
        [1.0, 0.8, 0.8, 1.0, 1.0, 0.8, 0.8, 1.0]
    );
    require_sticky_work_refusal("catia_a5_weight_mirror_copy", |ctx| {
        super::super::a5_weights(ctx, &bytes, &mut 0, 2, 4, bytes.len())
    });
}

#[test]
fn explicit_weight_program_refuses_lane_read_work() {
    let mut bytes = vec![0x00];
    for weight in [1.0_f64, 2.0, 3.0, 4.0] {
        bytes.extend_from_slice(&weight.to_le_bytes());
    }
    require_work_operations(
        |ctx| super::super::a5_weights(ctx, &bytes, &mut 0, 2, 2, bytes.len()),
        &["catia_a5_explicit_weights"],
    );
}

#[test]
fn grid_rows_charge_each_row_and_value_probe() {
    let bytes: Vec<u8> = [1.0_f64, 2.0, 3.0, 4.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect();
    let read = |ctx: &DecodeContext<'_>| {
        super::super::read_weight_rows(ctx, &bytes, 0, 2, 2, "test grid rows")
    };
    // Three row probes and three value probes per row use nine work units.
    let rows = crate::test_support::with_work_limit(9, read)
        .expect("two rows and four values")
        .expect("complete grid");
    assert_eq!(
        rows.concat()
            .iter()
            .map(|weight| weight.get())
            .collect::<Vec<_>>(),
        [1.0, 2.0, 3.0, 4.0]
    );
    crate::test_support::with_work_limit(6, |ctx| {
        let Err(CodecError::ResourceLimit(limit)) = read(ctx) else {
            panic!("the last value must refuse")
        };
        assert_eq!(limit.operation, "test grid rows");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn grid_rows_stop_at_the_first_invalid_value() {
    let bytes: Vec<u8> = [0.0_f64, 2.0, 3.0, 4.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect();
    // One row and one rejected value use two work units.
    assert!(crate::test_support::with_work_limit(2, |ctx| {
        super::super::read_weight_rows(ctx, &bytes, 0, 2, 2, "test grid rows")
    })
    .expect("rejection fits two units")
    .is_none());
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
                &mut super::super::A8ExternalGridSites::default(),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        },
        &[
            "catia_a8_external_grid_weight_scan",
            "catia_a8_external_weights",
        ],
    );
    let surface = crate::test_support::with_service_context(|ctx| {
        super::super::a8_surface_from_external_grid(
            ctx,
            &bytes,
            &header,
            &mut super::super::A8ExternalGridSites::default(),
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
        super::super::parse_surface_tail(&short, 0, short.len()),
        Some(short.len())
    );
    short[71..79].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(
        super::super::parse_surface_tail(&short, 0, short.len()),
        None
    );
    let mut long = a5_surface_tail();
    long[71..79].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(
        super::super::parse_surface_tail(&long, 0, long.len()),
        Some(long.len())
    );
    long[71..79].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(super::super::parse_surface_tail(&long, 0, long.len()), None);
}

#[test]
fn a5_distinct_knots_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 0, "catia_a5_distinct_knots");
}

#[test]
fn a5_expanded_knots_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 6, "catia_a5_expanded_knots");
}
