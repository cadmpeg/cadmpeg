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

/// Exercise package admission, structure-member opening, and dialect
/// classification.
pub fn package<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<(), CodecError> {
    let package = crate::package::Package::open(ctx, root)?;
    let _classification = crate::dialect::classify(package.structure(ctx)?);
    Ok(())
}

/// Exercise the structure-member decoder: header, directory, GZIP streams,
/// element grammar, string pool, definitions, occurrences, properties, and
/// the XML form.
pub fn structure(ctx: &DecodeContext<'_>, member: View<'_>) -> Result<(), CodecError> {
    crate::pvs::read(ctx, member).map(|_| ())
}

/// Exercise the geometry-member decoder: header, directory, preamble, GZIP
/// streams, TLV framing, appearances, surface and edge tessellation, and the
/// exact-stream accessor.
pub fn geometry(ctx: &DecodeContext<'_>, member: View<'_>) -> Result<(), CodecError> {
    crate::ol::decode(ctx, member)?;
    crate::ol::exact_stream(ctx, member).map(|_| ())
}

/// Exercise the structure and geometry member signature readers.
pub fn signatures(data: &[u8]) {
    let member = View::over_retained(data);
    let _probe = crate::pvs::generation(member);
    let _probe = crate::ol::generation(member);
}
