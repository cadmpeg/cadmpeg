// SPDX-License-Identifier: Apache-2.0
//! IGES codec. Decode admits every declared version and applies
//! version-specific envelope rules; semantic rules are verified for versions
//! 5.1, 5.2, and 5.3.
//!
//! <!-- generated: capability -->
//! Support: depth L9, breadth 2 of 21 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#iges)).
//! <!-- /generated: capability -->
//!
//! Compressed ASCII and Binary read normalization is provided. Bounded
//! semantic writing and independent-producer acceptance are part of the
//! verified profile.

mod binary;
mod card;
mod compressed;
mod dialect;
mod directory;
mod entities;
mod error;
mod global;
mod graph;
/// Byte-offset constants generated from `docs/layouts/iges.toml`.
pub(crate) mod layout;
mod loss;
mod native;
mod parameter;
mod profile;
mod reader;
mod representation;
mod writer;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeOptions, DecodeResult, EncodeInput, Encoder, ExportPlan,
    TargetDescriptor, TargetRequest,
};
use cadmpeg_ir::hash::document_local_sha256;
use cadmpeg_ir::CadIr;
use std::io::Cursor;

pub(crate) const SOURCE_IMAGE_ID: &str = "iges:file:source-image#0";

/// IGES specification version selected for semantic output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IgesVersion {
    /// IGES 5.3 Fixed ASCII.
    #[default]
    V5_3,
    /// IGES 5.2 Fixed ASCII.
    V5_2,
    /// IGES 5.1 Fixed ASCII.
    V5_1,
    /// IGES 5.0 Fixed ASCII.
    V5_0,
    /// IGES 4.0 Fixed ASCII.
    V4_0,
}

impl IgesVersion {
    /// Every version this writer can emit, in registry order.
    pub(crate) const ALL: [Self; 5] = [Self::V4_0, Self::V5_0, Self::V5_1, Self::V5_2, Self::V5_3];

    /// The registry dialect id this version writes.
    ///
    /// The spelling a caller passes as `TargetRequest::Explicit`, and the
    /// value `ExportReport::target` carries after a synthesis at this version.
    #[must_use]
    pub const fn target(self) -> &'static str {
        dialect::IgesDialect::fixed_ascii(self).pinned()
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::V4_0 => "4.0",
            Self::V5_0 => "5.0",
            Self::V5_1 => "5.1",
            Self::V5_2 => "5.2",
            Self::V5_3 => "5.3",
        }
    }

    pub(crate) const fn global_flag(self) -> u8 {
        match self {
            Self::V4_0 => 6,
            Self::V5_0 => 8,
            Self::V5_1 => 9,
            Self::V5_2 => 10,
            Self::V5_3 => 11,
        }
    }
}

/// Codec for IGES files.
#[derive(Debug, Default, Clone, Copy)]
pub struct IgesCodec;

pub(crate) fn document_digest(ir: &CadIr) -> String {
    document_local_sha256(ir, "iges", SOURCE_IMAGE_ID)
}

impl CodecBackend for IgesCodec {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        representation::confidence(prefix)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let mut reader = Cursor::new(root.window());
        let representation = representation::classify(&mut reader)?;
        match representation {
            representation::Representation::FixedAscii => {
                reader::inspect(ctx, root.window(), representation, root.window().len())
            }
            representation::Representation::CompressedAscii => {
                let normalized = compressed::normalize(root.window(), Some(ctx))?;
                reader::inspect(ctx, &normalized, representation, root.window().len())
            }
            representation::Representation::Binary => {
                let normalized = binary::normalize(root.window(), Some(ctx))?;
                reader::inspect(ctx, &normalized, representation, root.window().len())
            }
            representation::Representation::Unknown => Err(CodecError::WrongFormat(
                "unrecognized IGES representation".into(),
            )),
        }
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let mut source = Cursor::new(root.window());
        let representation = representation::classify(&mut source)?;
        let options = DecodeOptions {
            container_only: ctx.container_only(),
            policy: *ctx.policy(),
        };
        match representation {
            representation::Representation::FixedAscii => {
                reader::decode(root.window(), root.window(), representation, options, ctx)
            }
            representation::Representation::CompressedAscii => {
                let normalized = compressed::normalize(root.window(), Some(ctx))?;
                reader::decode(&normalized, root.window(), representation, options, ctx)
            }
            representation::Representation::Binary => {
                let normalized = binary::normalize(root.window(), Some(ctx))?;
                reader::decode(&normalized, root.window(), representation, options, ctx)
            }
            representation::Representation::Unknown => Err(CodecError::WrongFormat(
                "unrecognized IGES representation".into(),
            )),
        }
    }
}

/// IGES encoder.
///
/// Carries no target state. Which dialect an export writes is
/// [`TargetRequest`]'s answer, resolved against the source: an explicit target
/// names it, `Inherit` preserves the source's own, and a document with nothing
/// to inherit falls to the catalog default. An encoder-held version would be a
/// fourth answer, and the one that used to override the other three.
#[derive(Debug, Clone, Copy, Default)]
pub struct IgesEncoder;

impl Encoder for IgesEncoder {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        dialect::TARGETS
    }

    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        writer::target::plan(input, request)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
