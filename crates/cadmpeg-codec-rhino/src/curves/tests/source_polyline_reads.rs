// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::SourceFidelity;

use crate::curves::{decode_polyline_view, DecodedGeometry, GeometryError};
use crate::settings::MillimeterScale;
use crate::test_support::test_archive::polyline_payload;

#[test]
fn nonzero_source_polyline_window_credits_only_its_native_fields() {
    let mut bytes = b"prefix!".to_vec();
    let start = bytes.len();
    let payload = polyline_payload(
        &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0]],
        &[-2.0, 0.5, 6.0],
    );
    bytes.extend(&payload);
    let fields_end = bytes.len();
    bytes.extend(b"unparsed-extension");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("genuine root source");
    let source = root
        .child(start, bytes.len())
        .expect("nonzero polyline window");
    let parsed = ctx
        .record("polyline", source, || {
            Ok(decode_polyline_view(
                &ctx,
                source,
                start..bytes.len(),
                MillimeterScale::IDENTITY,
            ))
        })
        .expect("capture scope")
        .expect("native polyline fields");
    let DecodedGeometry::Curve { curve } = parsed else {
        panic!("native polyline leaf")
    };
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = curve.reported_geometry() else {
        panic!("native polyline NURBS carrier")
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [-2.0, -2.0, 0.5, 6.0, 6.0]);
    assert_eq!(
        nurbs
            .control_points()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(3.0, 2.0, 0.0)
        ]
    );
    assert!(nurbs.weights().is_none());
    assert!(curve.parameter_range().is_none());
    let DecodedGeometry::Curve { curve: copied } = decode_polyline_view(
        &ctx,
        View::over_retained(&bytes),
        start..bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect("same detached grammar") else {
        panic!("same polyline leaf")
    };
    assert_eq!(copied.reported_geometry(), curve.reported_geometry());
    assert_eq!(copied.parameter_range(), curve.parameter_range());
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("actual captures");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()["polyline"];
    assert_eq!(layout.offset(), u64::try_from(start).expect("small start"));
    assert_eq!(
        layout.byte_len(),
        u64::try_from(bytes.len() - start).expect("small extent")
    );
    assert_eq!(
        layout.reads()["file"],
        [u64::try_from(start).expect("small start")
            ..u64::try_from(fields_end).expect("small fields end")]
    );
    assert!(fields_end < bytes.len());
}

#[test]
fn unbacked_polyline_counts_fail_before_allocating_the_claimed_points() {
    for count in [-1_i32, i32::MAX] {
        let mut bytes = vec![0x10];
        bytes.extend(count.to_le_bytes());
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
        let failure = decode_polyline_view(
            &ctx,
            View::over_retained(&bytes),
            0..bytes.len(),
            MillimeterScale::IDENTITY,
        )
        .expect_err("unbacked point count cannot allocate");
        assert!(
            matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength { offset: 1, value }) if value == i128::from(count))
        );
        ctx.charge_entities(1, "valid sibling after malformed count")
            .expect("malformed input is not a fabricated resource refusal");
    }
}

#[test]
fn polyline_source_entry_preserves_the_actual_caller_depth_allowance() {
    let bytes = polyline_payload(&[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0]], &[10.0, 12.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root source");
    let failure = decode_polyline_view(&ctx, source, 0..bytes.len(), MillimeterScale::IDENTITY)
        .expect_err("caller depth allowance is not reset");
    let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = failure else {
        panic!("real caller resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    let capture_failure = ctx
        .take_record_captures()
        .expect_err("pending caller refusal prevents releasing record captures");
    let cadmpeg_core::CodecError::ResourceLimit(persisted) = capture_failure else {
        panic!("capture extraction must preserve the caller resource refusal")
    };
    assert_eq!(persisted, limit);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
