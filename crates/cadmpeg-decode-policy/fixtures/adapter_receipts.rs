// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::iter_source::IncrementalSource;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;

pub fn visit_dialect_layers(
    ctx: &DecodeContext<'_>,
    layers: &DialectLayers,
) -> Result<(), CodecError> {
    for layer in ctx.admit_iter(
        IncrementalSource::new(layers.iter()),
        "visit classified dialect layers",
    )? {
        let _layer = layer?;
    }
    Ok(())
}
