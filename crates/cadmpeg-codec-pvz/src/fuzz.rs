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

/// Exercise the exact-geometry reader and B-rep writer over an expanded
/// exact-stream payload. The stream identity follows the version stored in
/// the payload's first record.
pub fn exact_geometry<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<(), CodecError> {
    let stream_id = match View::u32_le_at(root.window(), 8) {
        Some(4) => crate::brep::EXACT_STREAM_F64,
        _ => crate::brep::EXACT_STREAM_F32,
    };
    let exact = crate::brep::parse(ctx, stream_id, root)?;
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let mut losses = Vec::new();
    let stream = cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("exact"));
    let _body = crate::brep::emit(
        ctx,
        &exact,
        &cadmpeg_ir::identity_key!("fuzz"),
        &mut crate::brep::emit::Sink {
            model: &mut ir.model,
            annotations: &mut annotations,
            losses: &mut losses,
            stream: &stream,
        },
    )?;
    Ok(())
}
