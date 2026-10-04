// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;

pub fn visit_dialect_layers(
    ctx: &DecodeContext<'_>,
    layers: &DialectLayers,
) -> Result<(), CodecError> {
    for layer in ctx.admit_iter(layers, "visit classified dialect layers")? {
        let _layer = layer;
    }
    Ok(())
}

pub fn unadmitted_dialect_layers(_ctx: &DecodeContext<'_>, layers: &DialectLayers) {
    for layer in layers.iter() { // finding: uncharged_decode_work
        let _layer = layer;
    }
}
