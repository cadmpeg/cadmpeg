// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::CodecBackend;

fn with_source(data: &[u8], run: impl FnOnce(&DecodeContext<'_>, cadmpeg_core::decode::View<'_>)) {
    let arena = DecodeArena::new();
    let Ok((ctx, source)) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())
    else {
        return;
    };
    run(&ctx, source);
}

/// Exercise the ASM binary SAB decode path.
pub fn decode_asm_binary(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::decode::decode_asm_binary(ctx, source.window());
    });
}

/// Exercise the Spatial ACIS binary SAB decode path.
pub fn decode_acis_binary(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::decode::decode_acis_binary(ctx, source.window());
    });
}

/// Exercise container inspection over a bare ASM stream.
pub fn inspect(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::SatCodec.inspect_impl(ctx, source);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::decode_asm_binary(&[]);
        super::decode_acis_binary(&[]);
        super::inspect(&[]);
    }
}
