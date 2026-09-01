// SPDX-License-Identifier: Apache-2.0
//! IGES codec. Decode admits every declared version and applies
//! version-specific envelope rules; semantic rules are verified for versions
//! 5.1, 5.2, and 5.3.
//!
//! <!-- generated: capability -->
//! Support: L9 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#iges)).
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
mod version;
mod writer;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeOptions, DecodeResult, EncodeInput, Encoder, ExportPlan,
    TargetRequest,
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

    /// The generic encoder view, projected from this typed write vocabulary.
    /// The writer emits only the five verified Fixed ASCII rows. Each bare
    /// version is an alias, and 5.3 is the cross-format default.
    pub(crate) const TARGETS: &'static [TargetDescriptor] = &[
        Self::V4_0.descriptor(),
        Self::V5_0.descriptor(),
        Self::V5_1.descriptor(),
        Self::V5_2.descriptor(),
        Self::V5_3.descriptor(),
    ];

    const fn target(self) -> &'static str {
        dialect::IgesDialect::fixed_ascii(self).pinned()
    }

    pub(crate) fn from_target(target: &TargetDescriptor) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|version| version.descriptor().id == target.id)
    }

    /// The typed write-target catalog row for this version.
    #[must_use]
    pub const fn descriptor(self) -> TargetDescriptor {
        let (aliases, default) = match self {
            Self::V4_0 => (&["4.0"].as_slice(), false),
            Self::V5_0 => (&["5.0"].as_slice(), false),
            Self::V5_1 => (&["5.1"].as_slice(), false),
            Self::V5_2 => (&["5.2"].as_slice(), false),
            Self::V5_3 => (&["5.3"].as_slice(), true),
        };
        TargetDescriptor {
            id: DialectId::pinned(self.target()),
            aliases,
            default,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        version::VersionFlag::from_write_version(self).name()
    }

    pub(crate) const fn global_flag(self) -> u8 {
        version::VersionFlag::from_write_version(self).value() as u8
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

impl Encoder for IgesCodec {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        IgesVersion::TARGETS
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        writer::target::plan(input, request)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
