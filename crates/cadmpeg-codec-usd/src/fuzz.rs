// SPDX-License-Identifier: Apache-2.0
//! Parser probes for the `cadmpeg-fuzz` targets.
//!
//! Context-taking probes return `Result<(), CodecError>`. They discard
//! successful parser values and propagate parser and resource errors.
//! Context-independent probes return `()` and discard their results. Every
//! probe must accept arbitrary bytes without panicking.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Exercise header detection and dialect classification.
pub fn header(data: &[u8]) {
    if let Some(header) = crate::detect::header(data) {
        let _classification = crate::dialect::classify(&header).admit();
    }
}

/// Exercise the USDA grammar over a whole layer.
pub fn layer(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<(), CodecError> {
    crate::text::grammar::parse_layer(ctx, root.window()).map(|_| ())
}

/// Exercise the USDC reader over a whole layer.
pub fn crate_layer(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<(), CodecError> {
    crate::usdc::read(ctx, root).map(|_| ())
}

/// Exercise package admission and the root layer reader.
pub fn package(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<(), CodecError> {
    let mut source = crate::usdz::source::source(ctx, root)?;
    crate::usdz::source::parse(ctx, &mut source).map(|_| ())
}
