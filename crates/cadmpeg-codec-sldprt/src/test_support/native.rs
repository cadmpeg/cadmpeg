// SPDX-License-Identifier: Apache-2.0
//! Native-namespace load and store helpers for crate tests.
#![allow(clippy::unwrap_used)]

/// Loads `namespace` in a session of its own under `limits`.
pub(crate) fn load_under(
    namespace: &cadmpeg_ir::NativeNamespace,
    limits: cadmpeg_core::decode::ResourceLimits,
    linear: cadmpeg_ir::scalar::PositiveLength,
) -> Result<crate::native::SldprtNative, cadmpeg_ir::NativeConvertError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &cadmpeg_core::decode::DecodePolicy { limits },
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    crate::native::SldprtNative::load_in(&ctx, namespace, linear)
}

pub(crate) fn sldprt_native(ir: &cadmpeg_ir::CadIr) -> crate::native::SldprtNative {
    load_under(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
        cadmpeg_core::decode::ResourceLimits::default(),
        ir.tolerances.linear,
    )
    .unwrap()
}

pub(crate) fn update_sldprt_native<R>(
    ir: &mut cadmpeg_ir::CadIr,
    update: impl FnOnce(&mut crate::native::SldprtNative) -> R,
) -> R {
    let mut native = sldprt_native(ir);
    let result = update(&mut native);
    native
        .store(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            ir.native.namespace_mut(cadmpeg_ir::codec_id!("sldprt")),
        )
        .unwrap();
    result
}
