// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::SourceFidelity;

use crate::curves::{decode_nurbs_view, DecodedGeometry, GeometryError};
use crate::settings::MillimeterScale;
use crate::test_support::test_archive::rational_nurbs_curve_source;

#[test]
fn nonzero_source_nurbs_window_keeps_exact_lanes_and_skips_reserved_bounds_and_suffix() {
    let fixture = rational_nurbs_curve_source(0x11, [2.0, 4.0]);
    let mut bytes = b"prefix!".to_vec();
    let start = bytes.len();
    bytes.extend(&fixture.bytes);
    let fields_end = bytes.len();
    bytes.extend(b"unparsed-extension");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("genuine root source");
    let source = root
        .child(start, bytes.len())
        .expect("nonzero NURBS window");
    let parsed = ctx
        .record("nurbs", source, || {
            Ok(decode_nurbs_view(
                &ctx,
                source,
                start..bytes.len(),
                MillimeterScale::IDENTITY,
            ))
        })
        .expect("capture scope")
        .expect("native NURBS fields");
    let DecodedGeometry::Curve { curve } = parsed else {
        panic!("native NURBS leaf")
    };
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = curve.reported_geometry() else {
        panic!("NURBS class stays NURBS")
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        nurbs
            .control_points()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)]
    );
    assert_eq!(
        nurbs
            .weights()
            .expect("rational lane")
            .into_iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        [2.0, 4.0]
    );
    assert!(!nurbs.periodic());
    assert!(curve.parameter_range().is_none());
    let DecodedGeometry::Curve { curve: copied } = decode_nurbs_view(
        &ctx,
        View::over_retained(&bytes),
        start..bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect("same detached grammar") else {
        panic!("same NURBS leaf")
    };
    assert_eq!(copied.reported_geometry(), curve.reported_geometry());
    assert_eq!(copied.parameter_range(), curve.parameter_range());
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("one actual extraction");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()["nurbs"];
    assert_eq!(layout.offset(), u64::try_from(start).expect("small start"));
    assert_eq!(
        layout.byte_len(),
        u64::try_from(bytes.len() - start).expect("small extent")
    );
    assert_eq!(
        layout.reads()["file"],
        [
            u64::try_from(start).expect("small start")
                ..u64::try_from(start + fixture.reserved_bounds.start).expect("small bounds start"),
            u64::try_from(start + fixture.reserved_bounds.end).expect("small bounds end")
                ..u64::try_from(fields_end).expect("small fields end")
        ]
    );
    assert!(fields_end < bytes.len());
}

#[test]
fn unbacked_nurbs_knot_counts_fail_before_allocating_the_claimed_lane() {
    for count in [-1_i32, i32::MAX] {
        let fixture = rational_nurbs_curve_source(0x10, [2.0, 4.0]);
        let mut bytes = fixture.bytes[..fixture.knot_count_offset].to_vec();
        bytes.extend(count.to_le_bytes());
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
        let failure = decode_nurbs_view(
            &ctx,
            View::over_retained(&bytes),
            0..bytes.len(),
            MillimeterScale::IDENTITY,
        )
        .expect_err("unbacked knot count cannot allocate");
        assert!(
            matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength { offset, value }) if offset == fixture.knot_count_offset && value == i128::from(count))
        );
        ctx.charge_entities(1, "valid sibling after malformed knot count")
            .expect("malformed count is not a fabricated resource refusal");
    }
}

#[test]
fn nurbs_source_entry_preserves_the_actual_caller_depth_allowance_and_refusal() {
    let fixture = rational_nurbs_curve_source(0x10, [2.0, 4.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, source) =
        DecodeContext::from_root_bytes(&fixture.bytes, &arena, &policy).expect("root source");
    let failure = decode_nurbs_view(
        &ctx,
        source,
        0..fixture.bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect_err("caller depth allowance is not reset");
    let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = failure else {
        panic!("real caller resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    let cadmpeg_core::CodecError::ResourceLimit(persisted) = ctx
        .take_record_captures()
        .expect_err("pending caller refusal prevents releasing captures")
    else {
        panic!("capture extraction preserves the same resource refusal")
    };
    assert_eq!(persisted, limit);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
