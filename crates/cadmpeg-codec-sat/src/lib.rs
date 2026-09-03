// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Decode bare Autodesk `ShapeManager` (ASM) B-rep streams.
//!
//! A bare stream is an ASM serialization outside any container: a binary
//! `.sab`/`.smb` SAB stream or a text `.sat`/`.smt` stream. Content
//! selects the path, never the file extension: the `ASM BinaryFile` magic
//! selects the binary framer and the ASCII header lines select the text
//! parser. Both paths decode through the shared kernel decoders in
//! [`cadmpeg_asm::brep`] into the neutral model arenas, with the kernel-side
//! native records under the `sat` namespace.
//!
//! Spatial ACIS 217 and 218 binary streams use the verified 32-bit SAB grammar.
//! Other ACIS binary header bands keep an admitted `sat:` host layer and recover
//! through an unverified `acis:` kernel layer that names the nearest
//! verified grammar and charges the recovery loss. Inspection and decode emit
//! both layers. A text stream frames on either branch terminator, and its decode
//! outcome decides whether the report carries geometry.
//!
//! <!-- generated: capability sat -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#asmacis-bare-satsmtsmbsab-streams)).
//! <!-- /generated: capability sat -->

mod coverage;
mod decode;
mod detect;
mod dialect;
#[allow(dead_code)] // Loss catalog is consumed by tests.
mod loss;

include!("dialect/registry_ids.rs");

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded};
use cadmpeg_ir::ContainerSummary;

/// Bare ASM stream codec.
pub struct SatCodec;

impl CodecBackend for SatCodec {
    const FORMAT: &'static str = FORMAT;

    fn detect_impl(&self, prefix: &[u8]) -> Confidence {
        detect::confidence(prefix)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        detect::inspect(ctx, root)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root.window())
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
pub(crate) mod test_support;
