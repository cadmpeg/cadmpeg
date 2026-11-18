// SPDX-License-Identifier: Apache-2.0
//! Parser probes for the `cadmpeg-fuzz` targets.
//!
//! Context-taking probes return `Result<(), CodecError>`. They discard
//! successful parser values and propagate parser and resource errors. Every
//! probe must accept arbitrary bytes without panicking.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Exercise file opening, document parsing, classification, and the blob
/// audit.
pub fn source<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<(), CodecError> {
    let source = crate::source::Source::open(ctx, root)?;
    let _classification = source.classify();
    source.audit_blobs(ctx).map(|_| ())
}

/// Exercise the `SCBREP05` and `SCMESH01` body slice parsers and the
/// neutral B-rep emitter on one slice.
pub fn cache_body<'a>(ctx: &DecodeContext<'a>, slice: View<'a>) -> Result<(), CodecError> {
    crate::cache::probe_body(ctx, slice)
}

/// Exercise the content detector.
pub fn detect(ctx: &DecodeContext<'_>, prefix: &[u8]) -> Result<(), CodecError> {
    crate::detect::confidence(ctx, prefix).map(|_| ())
}
