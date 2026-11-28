// SPDX-License-Identifier: Apache-2.0
//! Layer decode: classification, parsing, and the neutral model.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::SourceFidelity;

/// Source attribute holding the metres per scene unit.
const ATTRIBUTE_METERS_PER_UNIT: &str = "meters_per_unit";
/// Source attribute naming the default prim.
const ATTRIBUTE_DEFAULT_PRIM: &str = "default_prim";

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let mut source = crate::usdz::source::source(ctx, root)?;
    let layer = crate::usdz::source::parse(ctx, &mut source)?;
    let metrics = crate::model::metrics(&layer);
    let mut attributes = BTreeMap::from([(
        cadmpeg_core::nonblank_const!(ATTRIBUTE_METERS_PER_UNIT),
        metrics.meters_per_unit.to_string(),
    )]);
    if let Some(default_prim) = &metrics.default_prim {
        attributes.insert(
            cadmpeg_core::nonblank_const!(ATTRIBUTE_DEFAULT_PRIM),
            default_prim.clone(),
        );
    }
    let mut ir = CadIr::decoded(SourceMeta::classified(source.layers, attributes));
    ir.up_axis = Some(metrics.up_axis);
    let mut body = DecodeBody {
        losses: source.losses,
        ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), false))
    };
    if !ctx.container_only() {
        let layers = crate::layers::load(ctx, layer, source.package.take())?;
        let transferred = crate::model::build(ctx, &layers, &metrics, &mut ir, &mut body)?;
        body.transfer = DecodeTransfer::at(ctx.depth(), transferred);
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity: SourceFidelity::for_root(root),
    })
}
