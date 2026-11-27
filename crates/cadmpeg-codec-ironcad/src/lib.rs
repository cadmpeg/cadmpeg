// SPDX-License-Identifier: Apache-2.0
//! Reads `IronCAD` scene files.
//!
//! <!-- generated: capability ironcad -->
//! Support: L0 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#ironcad-ics)).
//! <!-- /generated: capability ironcad -->

mod acis;
mod container;
mod decode;
mod dialect;
mod frame;
mod kernel;
mod layout;
mod loss;
mod tree;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Read-only `IronCAD` scene codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct IronCadCodec;

impl CodecBackend for IronCadCodec {
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
