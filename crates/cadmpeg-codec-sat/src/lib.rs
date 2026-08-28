// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Decode bare Autodesk `ShapeManager` (ASM) B-rep streams.
//!
//! A bare stream is an ASM serialization outside any container: a binary
//! `.smb`/`.smbh`-style SAB stream or a text `.sat`/`.smt` stream. Content
//! selects the path, never the file extension: the `ASM BinaryFile` magic
//! selects the binary framer and the ASCII header lines select the text
//! parser. Both paths decode through the shared kernel decoders in
//! [`cadmpeg_asm::brep`] into the neutral model arenas, with the kernel-side
//! native records under the `sat` namespace.
//!
//! Spatial ACIS 217 and 218 binary streams use the verified 32-bit SAB grammar.
//! Other ACIS binary header bands recover through the nearest verified grammar,
//! report admitted-unverified, and charge the recovery loss. Inspection and
//! decode emit the primary `sat:` stream layer and the shared `acis:` kernel
//! layer. A text stream frames on either branch terminator, and its decode
//! outcome decides whether the report carries geometry.
//!
//! <!-- generated: capability -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#asmacis-bare-satsmtsmbsab-streams)).
//! <!-- /generated: capability -->

mod decode;
mod detect;
mod dialect;
#[allow(dead_code)] // Loss catalog is consumed by tests.
mod loss;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{CodecBackend, Confidence, DecodeResult};

/// The stable format identifier and native namespace.
pub(crate) const FORMAT: &str = "sat";

/// Bare ASM stream codec.
pub struct SatCodec;

impl CodecBackend for SatCodec {
    fn id(&self) -> &'static str {
        FORMAT
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        detect::confidence(prefix)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        detect::inspect(ctx, root)
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root.window())
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
pub(crate) mod test_support;
