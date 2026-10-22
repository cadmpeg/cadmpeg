// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::SourceFidelity;

use crate::curves::GeometryError;
use crate::settings::MillimeterScale;
use crate::surfaces::{decode_nurbs_view, DecodedSurface, TypedSurface};
use crate::test_support::test_archive::nurbs_surface_source;

#[test]
fn nonzero_source_surface_window_keeps_the_tensor_and_skips_bounds_and_suffix() {
    let fixture = nurbs_surface_source(Some([2.0, 4.0, 2.0, 4.0]));
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
        .expect("nonzero surface window");
    let parsed = ctx
        .record("surface", source, || {
            Ok(decode_nurbs_view(
                &ctx,
                source,
                start..bytes.len(),
                MillimeterScale::IDENTITY,
            ))
        })
        .expect("capture scope")
        .expect("native surface fields");
    let DecodedSurface::Typed {
        geometry: TypedSurface::Nurbs(surface),
        derived,
    } = parsed
    else {
        panic!("typed NURBS tensor")
    };
    assert!(derived);
    assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    assert_eq!(surface.u_knots().as_slice(), [-1.0, -1.0, 2.0, 2.0]);
    assert_eq!(surface.v_knots().as_slice(), [5.0, 5.0, 9.0, 9.0]);
    for (u, v, expected, weight) in [
        (0, 0, Point3::new(0.0, 0.0, 0.0), 2.0),
        (0, 1, Point3::new(0.0, 2.0, 0.0), 4.0),
        (1, 0, Point3::new(3.0, 0.0, 1.0), 2.0),
        (1, 1, Point3::new(3.0, 2.0, 1.0), 4.0),
    ] {
        assert_eq!(surface.pole(u, v).expect("source pole").get(), expected);
        assert_eq!(surface.weight(u, v).expect("source weight").get(), weight);
    }
    assert!(!surface.u_periodic());
    assert!(!surface.v_periodic());
    let DecodedSurface::Typed {
        geometry: TypedSurface::Nurbs(copied),
        derived: copied_derived,
    } = decode_nurbs_view(
        &ctx,
        View::over_retained(&bytes),
        start..bytes.len(),
        MillimeterScale::IDENTITY,
    )
    .expect("same detached grammar")
    else {
        panic!("same NURBS tensor")
    };
    assert_eq!(copied, surface);
    assert_eq!(copied_derived, derived);
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("one actual extraction");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()["surface"];
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
fn unbacked_surface_counts_fail_before_allocating_the_claimed_lane_or_grid() {
    for count in [-1_i32, i32::MAX] {
        let fixture = nurbs_surface_source(Some([2.0, 4.0, 2.0, 4.0]));
        for offset in [
            fixture.u_knot_count_offset,
            fixture.v_knot_count_offset,
            fixture.cv_count_offset,
        ] {
            let mut bytes = fixture.bytes[..offset].to_vec();
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
            .expect_err("unbacked count cannot allocate");
            assert!(
                matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength { offset: actual, value }) if actual == offset && value == i128::from(count))
            );
            ctx.charge_entities(1, "valid sibling after malformed surface count")
                .expect("malformed count is not a fabricated resource refusal");
        }
    }
}

#[test]
fn nurbs_surface_source_entry_preserves_the_caller_depth_allowance_and_refusal() {
    let fixture = nurbs_surface_source(None);
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
        panic!("capture extraction preserves the caller refusal")
    };
    assert_eq!(persisted, limit);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
