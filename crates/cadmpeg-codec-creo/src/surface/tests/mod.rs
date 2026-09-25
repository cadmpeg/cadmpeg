// SPDX-License-Identifier: Apache-2.0
mod contours;
mod dump;
mod inline;
mod planes;
mod positional;
mod round_envelopes;
mod rows;
mod scan;

const EPS_FRAME_COMPONENT: f64 = 1.0e-12;

fn with_decode_ctx<T>(
    bytes: &[u8],
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::desktop();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("surface fixture is admitted");
    run(&ctx).expect("surface fixture stays within resource limits")
}

fn named_prototype_records(
    payload: &[u8],
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> Vec<super::SurfacePrototypeRecord> {
    with_decode_ctx(payload, |ctx| {
        super::named_prototype_records(ctx, payload, refusals)
    })
}

fn named_surface_value(
    family: &super::SurfacePrototypeFamily,
    name: &str,
    body: &[u8],
    cache: &crate::scalar::ScalarCache,
    record: &dyn std::fmt::Display,
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> super::SurfaceNamedValue {
    with_decode_ctx(body, |ctx| {
        super::named_surface_value(ctx, family, name, body, cache, record, refusals)
    })
}

fn parameter_records(payload: &[u8]) -> Vec<super::SurfaceParameterRecord> {
    with_decode_ctx(payload, |ctx| super::parameter_records(ctx, payload))
}

fn cross_section_parameter_records(payload: &[u8]) -> Vec<super::SurfaceParameterRecord> {
    with_decode_ctx(payload, |ctx| {
        super::cross_section_parameter_records(ctx, payload)
    })
}

fn contour_records(payload: &[u8]) -> Vec<super::SurfaceContourRecord> {
    with_decode_ctx(payload, |ctx| super::contour_records(ctx, payload))
}

fn plane_local_systems(payload: &[u8]) -> Vec<super::PlaneLocalSystem> {
    with_decode_ctx(payload, |ctx| super::plane_local_systems(ctx, payload))
}

fn plane_local_systems_for_rows(
    payload: &[u8],
    rows: &[super::SurfaceRow],
) -> Vec<super::PlaneLocalSystem> {
    with_decode_ctx(payload, |ctx| {
        super::plane_local_systems_for_rows(ctx, payload, rows)
    })
}

fn positional_spline_replay_prototype(
    payload: &[u8],
    rows: &[super::SurfaceRow],
    row: &super::SurfaceRow,
) -> Option<super::SurfacePrototypeRecord> {
    with_decode_ctx(payload, |ctx| {
        super::positional_spline_replay_prototype(ctx, payload, rows, row)
    })
}

fn positional_spline_replay_body_end(
    payload: &[u8],
    rows: &[super::SurfaceRow],
    row: &super::SurfaceRow,
    body_start: usize,
    body_limit: usize,
    cache: &crate::scalar::ScalarCache,
) -> Option<usize> {
    with_decode_ctx(payload, |ctx| {
        super::positional_spline_replay_body_end(
            ctx, payload, rows, row, body_start, body_limit, cache,
        )
    })
}
