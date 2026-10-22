// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::TAU;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::SourceFidelity;

use crate::curves::{decode_arc_view, DecodedGeometry};
use crate::settings::MillimeterScale;
use crate::test_support::test_archive::arc_payload;

#[test]
fn nonzero_source_circle_window_credits_only_its_native_arc_fields() {
    let mut bytes = b"prefix!".to_vec();
    let start = bytes.len();
    let payload = arc_payload([0.0, TAU], [0.0, TAU]);
    bytes.extend(&payload);
    let fields_end = bytes.len();
    bytes.extend(b"unparsed-extension");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("genuine root source");
    let source = root
        .child(start, bytes.len())
        .expect("nonzero circle window");
    assert!(source.has_source_tracking());
    let parsed = ctx
        .record("circle", source, || {
            Ok(decode_arc_view(
                &ctx,
                source,
                start..bytes.len(),
                MillimeterScale::IDENTITY,
            ))
        })
        .expect("capture scope")
        .expect("native arc fields");
    let DecodedGeometry::Curve { curve } = parsed else {
        panic!("native arc leaf")
    };
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = curve.reported_geometry()
    else {
        panic!("canonical full circle");
    };
    assert_eq!(circle.center().get(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(circle.radius().get(), 2.0);
    assert_eq!(*circle.frame().axis(), UnitVector3::Z_AXIS);
    assert_eq!(*circle.frame().reference(), UnitVector3::X_AXIS);
    assert!(curve.parameter_range().is_none());
    let detached = View::over_retained(&bytes);
    assert!(!detached.has_source_tracking());
    let DecodedGeometry::Curve { curve: copied } = decode_arc_view(
        &ctx,
        detached,
        start..bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect("same detached grammar") else {
        panic!("same arc leaf")
    };
    assert_eq!(copied.reported_geometry(), curve.reported_geometry());
    assert_eq!(copied.parameter_range(), curve.parameter_range());
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("actual captures");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()["circle"];
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
