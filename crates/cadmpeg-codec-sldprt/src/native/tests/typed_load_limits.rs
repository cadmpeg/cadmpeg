//! Resource admission for native loading under a caller context.

use super::{sldprt_with_body_and_history, triangle_body, Cursor, DecodeOptions, SldprtCodec};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::Codec;

fn typed_load_refusal(policy: DecodePolicy) -> cadmpeg_core::CodecError {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::native::SldprtNative::load_with_context(
            &service,
            namespace,
            cadmpeg_ir::units::Tolerances::default().linear
        )
        .unwrap(),
        crate::native::SldprtNative::load_under(
            namespace,
            cadmpeg_core::decode::ResourceLimits::default(),
            cadmpeg_ir::units::Tolerances::default().linear
        )
        .unwrap()
    );
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::native::SldprtNative::load_with_context(
        &limited,
        namespace,
        cadmpeg_ir::units::Tolerances::default().linear,
    )
    .unwrap_err()
    .into()
}

#[test]
fn native_load_refuses_nesting_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    assert!(
        matches!(typed_load_refusal(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth)
    );
}

#[test]
fn a_writer_load_refuses_past_the_callers_nesting_limit_with_the_record_root_added() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let mut limits = cadmpeg_core::decode::ResourceLimits::desktop();
    limits.max_recursion_depth = 0;
    let error: cadmpeg_core::CodecError = crate::native::SldprtNative::load_under(
        namespace,
        limits,
        cadmpeg_ir::units::Tolerances::default().linear,
    )
    .unwrap_err()
    .into();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth && limit.limit == 1),
        "{error:?}"
    );
}
