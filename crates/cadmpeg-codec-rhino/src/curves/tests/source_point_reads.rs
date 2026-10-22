// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::SourceFidelity;

use crate::curves::{decode_point_view, DecodedGeometry};
use crate::settings::MillimeterScale;
use crate::test_support::test_dump::point_payload;

#[test]
fn nonzero_source_point_window_credits_only_the_consumed_fields() {
    let mut bytes = b"prefix!".to_vec();
    let start = bytes.len();
    let payload = point_payload([1.25, -2.5, 3.75]);
    bytes.extend(&payload);
    let fields_end = bytes.len();
    bytes.extend(b"unparsed-extension");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("genuine root source");
    let source = root
        .child(start, bytes.len())
        .expect("nonzero source window");
    assert!(source.has_source_tracking());
    let point = ctx
        .record("point", source, || {
            Ok(decode_point_view(
                &ctx,
                source,
                start..bytes.len(),
                MillimeterScale::IDENTITY,
            ))
        })
        .expect("capture scope")
        .expect("point fields");
    let DecodedGeometry::Point { position, scaled } = point else {
        panic!("actual point variant");
    };
    assert_eq!(
        position.get(),
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    assert!(!scaled);
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("actual captures");
    let layout = &fidelity.record_layout()["point"];
    assert_eq!(layout.offset(), u64::try_from(start).expect("small start"));
    assert_eq!(
        layout.byte_len(),
        u64::try_from(bytes.len() - start).expect("small extent")
    );
    assert_eq!(
        layout.reads()["file"],
        [u64::try_from(start).expect("small point start")
            ..u64::try_from(fields_end).expect("small point end")]
    );
    let detached = View::over_retained(&bytes);
    assert!(!detached.has_source_tracking());
    let DecodedGeometry::Point {
        position: copied,
        scaled: copied_scaled,
    } = decode_point_view(
        &ctx,
        detached,
        start..bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect("same detached grammar")
    else {
        panic!("same point variant");
    };
    assert_eq!(copied, position);
    assert_eq!(copied_scaled, scaled);
}
