// SPDX-License-Identifier: Apache-2.0
//! IGES codec. Decode admits every declared version and applies
//! version-specific envelope rules; semantic rules are verified for versions
//! 5.1, 5.2, and 5.3.
//!
//! <!-- generated: capability iges -->
//! Support: L9 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#iges)).
//! <!-- /generated: capability iges -->
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
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeOptions, DecodeResult, EncodeInput, EncoderBackend,
    EncoderTargetDomain, ExportPlan, ResolvedEncoderTarget,
};
use cadmpeg_ir::hash::document_local_sha256;
use cadmpeg_ir::CadIr;
use cadmpeg_ir::ContainerSummary;
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

macro_rules! writer_vocabulary {
    ($(#[$all_meta:meta])* $count:literal; $($variant:ident),+ $(,)?) => {
        $(#[$all_meta])*
        pub(crate) const ALL: [Self; $count] = [$(Self::$variant),+];
        /// The generic encoder view projected from [`Self::ALL`].
        pub(crate) const TARGETS: &'static [TargetDescriptor] = &[
            $(Self::$variant.descriptor()),+
        ];
    };
}

impl IgesVersion {
    writer_vocabulary!(
        /// Every version this writer can emit, in registry order.
        ///
        /// The same invocation projects the generic encoder catalog, so adding
        /// a typed version cannot omit its target descriptor. The writer emits
        /// only the verified Fixed ASCII rows. Each bare version is an alias,
        /// and 5.3 is the cross-format default.
        5;
        V4_0,
        V5_0,
        V5_1,
        V5_2,
        V5_3
    );

    pub(crate) fn from_catalog_entry(target: &TargetDescriptor) -> Self {
        Self::ALL
            .into_iter()
            .find(|version| version.descriptor().id == target.id)
            .expect("IGES target catalog is projected from IgesVersion::ALL")
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
            id: dialect::fixed_ascii_id(self),
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

impl EncoderBackend for IgesCodec {
    const FORMAT: &'static str = dialect::FORMAT;
    const TARGET_DOMAIN: EncoderTargetDomain = EncoderTargetDomain::Catalog(IgesVersion::TARGETS);

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError> {
        let ResolvedEncoderTarget::Native(resolved) = target else {
            unreachable!("a catalog encoder receives only native target resolutions")
        };
        writer::target::plan(input, &resolved)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
