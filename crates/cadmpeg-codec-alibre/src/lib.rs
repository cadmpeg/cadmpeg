// SPDX-License-Identifier: Apache-2.0
//! Reads Alibre Design part files (`.AD_PRT`) and sheet-metal part files
//! (`.AD_SMP`) into [`cadmpeg_ir::CadIr`].
//!
//! An `.AD_PRT` is a Microsoft Compound File. [`AlibreCodec`] detects a part
//! from the `SAB_DATA` and `STEP_DATA` streams of the compound-file directory
//! and does not classify other compound files as Alibre. The `SAB_DATA` stream
//! holds the part body as a Spatial ACIS binary stream, which decodes through
//! `cadmpeg-asm` into the neutral B-rep. The `STEP_DATA` stream holds the
//! feature model as STEP Part 21 text, which decodes into the design layer:
//! one ordered timeline of datums, sketches, and features with their
//! parameters. The crate is read-only.
//!
//! <!-- generated: capability alibre -->
//! Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#alibre-design-adprt)).
//! <!-- /generated: capability alibre -->

mod container;
mod coverage;
mod decode;
mod design;
mod dialect;
#[doc(hidden)]
pub mod fuzz;
mod history;
mod loss;
mod part21;

use cadmpeg_container::compound::CompoundPrefixProbe;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

use dialect::registry_ids::FORMAT;

/// Read-only Alibre Design part codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlibreCodec;

impl CodecBackend for AlibreCodec {
    const FORMAT: FormatId = FormatId::new(FORMAT);

    fn detect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<Confidence, CodecError> {
        let probe = CompoundPrefixProbe::inspect(ctx, View::over_retained(prefix.window()))?;
        let CompoundPrefixProbe::DirectoryEvidence { paths, .. } = probe else {
            return Ok(Confidence::No);
        };
        Ok(if container::has_alibre_evidence(&paths) {
            Confidence::High
        } else {
            Confidence::No
        })
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let container = container::AlibreContainer::open(ctx, root)?;
        let sab = container.stream(ctx, container::SAB_STREAM)?;
        let kernel = dialect::kernel_stream(sab.window());
        let classification = dialect::classify(kernel.as_ref());
        container.summary(classification, kernel.as_ref())
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
