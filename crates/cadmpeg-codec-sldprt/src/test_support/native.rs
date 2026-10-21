// SPDX-License-Identifier: Apache-2.0
//! Native-namespace load and store helpers for crate tests.
#![allow(clippy::unwrap_used)]

pub(crate) fn sldprt_native(ir: &cadmpeg_ir::CadIr) -> crate::native::SldprtNative {
    crate::native::SldprtNative::load(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
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
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("sldprt"),
        )
        .unwrap();
    result
}

/// Read byte-backed statements through their retained source-fidelity boundary.
pub(crate) fn sldprt_native_with_source(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> crate::native::SldprtNative {
    crate::native::source::load(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
        fidelity,
    )
    .expect("source-fidelity admission")
}
