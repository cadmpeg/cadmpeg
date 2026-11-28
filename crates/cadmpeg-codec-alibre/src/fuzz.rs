// SPDX-License-Identifier: Apache-2.0
//! Parser probes for the `cadmpeg-fuzz` targets.
//!
//! A probe returns `Result<(), CodecError>`. It discards the parsed value and
//! propagates parser and resource errors. Every probe must accept arbitrary
//! bytes without panicking.
#![doc(hidden)]

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Exercise the kernel-stream classification, the identifier-table framing, the
/// extended-base normalization, and the shared ASM decoders over raw `SAB_DATA`
/// bytes, without the compound-file container around them.
pub fn sab_stream(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let Some(kernel) = crate::dialect::kernel_stream(bytes) else {
        return Ok(());
    };
    crate::decode::decode_kernel_stream(ctx, bytes, &kernel).map(|_| ())
}
