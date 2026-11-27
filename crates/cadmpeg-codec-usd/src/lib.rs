// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Decode and encode `OpenUSD` layers.
//!
//! A layer is a USDA text file or a USDC binary file, alone or as the root
//! layer of a USDZ package. [`UsdCodec`] detects both encodings from their
//! first bytes and classifies the layer dialect from its header. A USDC layer
//! is read into the same parsed form as a USDA layer. The layer is composed
//! within itself: selected variants, internal
//! references, and the prim hierarchy. Prims become product definitions and
//! occurrences; meshes become tessellations; NURBS patches and curves become
//! NURBS carriers. Arcs to other layers stay external prototypes.
//!
//! <!-- generated: capability usd -->
//! Support: L2 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#openusd-layers)).
//! <!-- /generated: capability usd -->

mod brep;
mod decode;
mod detect;
mod dialect;
mod ids;
mod inspect;
mod joint;
mod layers;
/// Byte-offset constants generated from `docs/layouts/usd.toml`.
mod layout;
mod loss;
mod model;
mod schema;
mod stage;
mod text;
mod usdc;
mod usdz;
mod writer;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// The write targets: the USDA text layer of each declared version, and a
/// USDZ package of a USDA 1.0 root layer.
const TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: dialect::USD_USDA_1_0,
        aliases: &["usda", "1.0"],
    },
    TargetDescriptor {
        id: dialect::USD_USDA_1_1,
        aliases: &["1.1"],
    },
    TargetDescriptor {
        id: dialect::USD_USDA_1_2,
        aliases: &["1.2"],
    },
    TargetDescriptor {
        id: dialect::USD_USDA_1_3,
        aliases: &["1.3"],
    },
    TargetDescriptor {
        id: dialect::USD_USDZ,
        aliases: &["usdz"],
    },
    TargetDescriptor {
        id: dialect::USD_USDC_0_8_0,
        aliases: &["usdc", "0.8.0"],
    },
];

/// The header version of the text layer of each target in [`TARGETS`].
const TARGET_VERSIONS: [&str; 6] = ["1.0", "1.1", "1.2", "1.3", "1.0", "1.0"];

/// Codec for `OpenUSD` layers.
#[derive(Debug, Default, Clone, Copy)]
pub struct UsdCodec;

impl CodecBackend for UsdCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn detect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<Confidence, CodecError> {
        if usdz::detect(ctx, prefix.window())? {
            return Ok(Confidence::High);
        }
        Ok(detect::confidence(prefix.window()))
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        inspect::inspect(ctx, root)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        let mut decoded = decode::decode(ctx, root)?;
        decoded.ir.state_root_definition(
            ctx,
            crate::ids::stage(),
            cadmpeg_ir::products::ExternalDocument::default(),
        )?;
        Ok(decoded)
    }
}

impl EncoderBackend for UsdCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    const NAME: &'static str = "USD";
    /// Writes topology, geometry, tessellations, appearances, and placements;
    /// design arenas are not read.
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(TARGETS, Some(0));

    fn plan_resolved(
        &self,
        ctx: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        let packaged = target
            .synthesized_row()
            .is_some_and(|(_, entry)| entry.id == dialect::USD_USDZ);
        let binary = target
            .synthesized_row()
            .is_some_and(|(_, entry)| entry.id == dialect::USD_USDC_0_8_0);
        let (mut body, members) = writer::plan(ctx, input, &target, packaged)?;
        if binary {
            let (bytes, left_out) = crate_layer(ctx, &body.bytes)?;
            body.bytes = bytes;
            body.losses
                .extend(cadmpeg_ir::codec::write::value_unwritable(
                    "USDC",
                    "leaves out the fields whose value has no USDC 0.8.0 form",
                    left_out,
                ));
            let metres = input
                .scene
                .meters_per_unit
                .map_or(1.0, cadmpeg_ir::scalar::PositiveReal::get);
            body.notes = vec![format!(
                "USDC 0.8.0 binary layer, {}",
                writer::units_note(metres)
            )];
        }
        if packaged {
            let (bytes, losses) = usdz::package_layer(&body.bytes, &members);
            body.bytes = bytes;
            body.losses.extend(losses);
            body.notes
                .push(format!("USDZ package, root layer {}", usdz::ROOT_MEMBER));
        }
        Ok(body)
    }
}

/// The USDC form of the written text layer `text`, and the fields left out
/// because their value has no USDC form.
///
/// The text is this writer's own, so it parses: only the encode session's
/// budget stops the parse.
fn crate_layer(ctx: &DecodeContext<'_>, text: &[u8]) -> Result<(Vec<u8>, Vec<String>), CodecError> {
    let layer = text::grammar::parse_layer(ctx, text).inspect_err(|error| {
        debug_assert!(
            matches!(error, CodecError::ResourceLimit(_)),
            "the written layer parses: {error}"
        );
    })?;
    if let Some(limit) = ctx.resource_refusal() {
        return Err(CodecError::ResourceLimit(limit));
    }
    usdc::write::write(&layer)
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
