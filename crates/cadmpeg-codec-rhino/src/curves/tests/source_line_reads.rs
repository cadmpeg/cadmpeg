// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::SourceFidelity;

use crate::curves::{decode_line_view, DecodedGeometry};
use crate::settings::MillimeterScale;
use crate::test_support::test_archive::line_payload;

#[test]
fn nonzero_source_line_window_credits_only_its_native_fields() {
    let mut bytes = b"prefix!".to_vec();
    let start = bytes.len();
    let payload = line_payload([1.0, -4.0, 3.0], [1.0, 4.0, 3.0], [-2.0, 6.0]);
    bytes.extend(&payload);
    let fields_end = bytes.len();
    bytes.extend(b"unparsed-extension");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("genuine root source");
    let source = root.child(start, bytes.len()).expect("nonzero line window");
    let parsed = ctx
        .record("line", source, || {
            Ok(decode_line_view(
                &ctx,
                source,
                start..bytes.len(),
                MillimeterScale::IDENTITY,
            ))
        })
        .expect("capture scope")
        .expect("line fields");
    let DecodedGeometry::Curve { curve } = parsed else {
        panic!("native curve leaf")
    };
    assert_eq!(
        curve.parameter_range().expect("line domain").endpoints(),
        [-2.0, 6.0]
    );
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = curve.reported_geometry() else {
        panic!("native bounded line carrier");
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [-2.0, -2.0, 6.0, 6.0]);
    assert_eq!(
        nurbs
            .control_points()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [Point3::new(1.0, -4.0, 3.0), Point3::new(1.0, 4.0, 3.0)]
    );
    let detached = View::over_retained(&bytes);
    let DecodedGeometry::Curve { curve: copied } = decode_line_view(
        &ctx,
        detached,
        start..bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect("same detached grammar") else {
        panic!("same curve leaf")
    };
    assert_eq!(copied.reported_geometry(), curve.reported_geometry());
    assert_eq!(copied.parameter_range(), curve.parameter_range());
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("actual captures");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()["line"];
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
}
