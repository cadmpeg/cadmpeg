// SPDX-License-Identifier: Apache-2.0
//! Reads Solid Edge compound documents.
//!
//! <!-- generated: capability solidedge -->
//! Support: L0 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solid-edge-par-psm-and-asm)).
//! <!-- /generated: capability solidedge -->

mod container;
mod decode;
mod dialect;
mod layout;
mod loss;
mod settings;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Read-only Solid Edge document codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct SolidEdgeCodec;

impl CodecBackend for SolidEdgeCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn detect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<Confidence, CodecError> {
        container::detect(ctx, prefix)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        container::summary(ctx, root)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
