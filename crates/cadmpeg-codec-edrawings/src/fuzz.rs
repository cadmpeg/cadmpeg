// SPDX-License-Identifier: Apache-2.0
//! Parser probes for the `cadmpeg-fuzz` targets.
//!
//! Context-taking probes return `Result<(), CodecError>`. They discard
//! successful parser values and propagate parser and resource errors. Every
//! probe must accept arbitrary bytes without panicking.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::dialect::Grammar;

/// Exercise package admission, model member opening, wrapper reading, and
/// dialect classification.
pub fn package<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<(), CodecError> {
    let package = crate::package::Package::open(ctx, root)?;
    let wrapper = crate::stream::read_wrapper(package.model(ctx)?)?;
    let _classification = crate::dialect::classify(Some(&wrapper.version));
    Ok(())
}

/// Exercise the `eModel` member decoder over raw member bytes: comment line,
/// wrapper records, the zlib body under the decompression caps, and the
/// model body seam.
pub fn stream<'a>(ctx: &DecodeContext<'a>, member: View<'a>) -> Result<(), CodecError> {
    let wrapper = crate::stream::read_wrapper(member)?;
    let grammar = crate::dialect::classify(Some(&wrapper.version)).grammar();
    let model = crate::stream::parse(ctx, member)?;
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut losses = Vec::new();
    crate::hsf::decode_body(ctx, model.body, grammar, &mut ir, &mut losses).map(|_| ())
}

/// Exercise the model stream walker, the scene collector, and the CADIR
/// emitter over raw inflated stream bytes, once under each grammar.
pub fn model<'a>(ctx: &DecodeContext<'a>, body: View<'a>) -> Result<(), CodecError> {
    for grammar in [
        Grammar::Resaved,
        Grammar::SolidWorks,
        Grammar::SolidWorksLate,
    ] {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut losses = Vec::new();
        crate::hsf::decode_body(ctx, body, grammar, &mut ir, &mut losses)?;
    }
    Ok(())
}
