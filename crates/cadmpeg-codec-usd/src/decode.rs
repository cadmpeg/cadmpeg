// SPDX-License-Identifier: Apache-2.0
//! Layer decode: classification, parsing, and the neutral model.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::SourceFidelity;

/// Source attribute naming the stage up axis.
const ATTRIBUTE_UP_AXIS: &str = "up_axis";
/// Source attribute holding the metres per scene unit.
const ATTRIBUTE_METERS_PER_UNIT: &str = "meters_per_unit";
/// Source attribute naming the default prim.
const ATTRIBUTE_DEFAULT_PRIM: &str = "default_prim";

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let mut source = crate::usdz::source::source(ctx, root)?;
    let layer = crate::usdz::source::parse(ctx, &mut source)?;
    let mut losses = source.losses;
    let metrics = crate::model::metrics(&layer);
    let mut attributes = BTreeMap::from([
        (
            cadmpeg_core::nonblank_const!(ATTRIBUTE_UP_AXIS),
            metrics.up_axis.clone(),
        ),
        (
            cadmpeg_core::nonblank_const!(ATTRIBUTE_METERS_PER_UNIT),
            metrics.meters_per_unit.to_string(),
        ),
    ]);
    if let Some(default_prim) = &metrics.default_prim {
        attributes.insert(
            cadmpeg_core::nonblank_const!(ATTRIBUTE_DEFAULT_PRIM),
            default_prim.clone(),
        );
    }
    let mut ir = CadIr::decoded(SourceMeta::classified(source.layers, attributes));
    let mut transferred = false;
    if !ctx.container_only() {
        let layers = crate::layers::load(ctx, layer, source.package.take())?;
        transferred = crate::model::build(ctx, &layers, &metrics, &mut ir, &mut losses)?;
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer {
                scope: ctx.depth().required_scope(),
                geometry_transferred: transferred,
            })
        },
        source_fidelity: SourceFidelity::for_decode(ctx, cadmpeg_ir::Annotations::default()),
    })
}
