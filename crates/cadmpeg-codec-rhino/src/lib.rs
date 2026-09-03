// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes Rhino `.3dm` files through [`cadmpeg_ir::document::CadIr`].
//!
//! <!-- generated: capability rhino -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#rhino-3dm)).
//! <!-- /generated: capability rhino -->
//!
//! The codec provides bounded 3DM container inspection, typed decoding, and
//! explicitly versioned native writing from neutral IR.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{Catalog, EncodeInput, EncoderBackend, ExportBody, ResolvedWrite};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded};
use cadmpeg_ir::ContainerSummary;

pub(crate) mod annotations;
pub(crate) mod brep;
pub(crate) mod cage;
pub(crate) mod chunks;
pub(crate) mod container;
pub(crate) mod coverage;
pub(crate) mod curve_on_surface;
pub(crate) mod curves;
pub(crate) mod decode;
pub(crate) mod detail;
pub(crate) mod dialect;
pub(crate) mod dimensions;
pub(crate) mod document_data;
pub(crate) mod extrusion;
pub(crate) mod hatch;
pub(crate) mod history;
pub(crate) mod instances;
/// Byte-offset constants generated from `docs/layouts/rhino.toml`.
pub(crate) mod layout;
pub(crate) mod legacy;
#[allow(dead_code)] // Loss catalog is consumed by the writer and hidden facade.
pub(crate) mod loss;
pub(crate) mod mesh;
pub(crate) mod mesh_modifiers;
pub(crate) mod morph;
pub(crate) mod objects;
pub(crate) mod polyedge;
pub(crate) mod presentation;
pub(crate) mod product;
pub(crate) mod settings;
pub(crate) mod subd;
pub(crate) mod surfaces;
pub(crate) mod views;
pub(crate) mod wire;
mod writer;

#[doc(hidden)]
pub mod fuzz;

const MAGIC: &[u8] = chunks::MAGIC;

/// Decoder and inspector for Rhino `.3dm` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct RhinoCodec;

/// A supported native 3DM output archive version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RhinoArchiveVersion {
    /// Rhino 5 archive (`50`).
    V5,
    /// Rhino 6 archive (`60`).
    V6,
    /// Rhino 7 archive (`70`).
    V7,
    /// Rhino 8 archive (`80`).
    V8,
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

impl RhinoArchiveVersion {
    writer_vocabulary!(
        /// Every archive version this writer can emit, in registry order.
        ///
        /// The same invocation projects the generic encoder catalog, so adding
        /// a typed version cannot omit its target descriptor. Archive words and
        /// Rhino majors are aliases; archive 80 is the cross-format default.
        4;
        V5,
        V6,
        V7,
        V8
    );

    /// The typed write-target catalog row for this archive version.
    #[must_use]
    pub const fn descriptor(self) -> TargetDescriptor {
        let (dialect, aliases) = match self {
            Self::V5 => (chunks::ArchiveVersion::V5, &["50"].as_slice()),
            Self::V6 => (chunks::ArchiveVersion::V6, &["6", "60"].as_slice()),
            Self::V7 => (chunks::ArchiveVersion::V7, &["7", "70"].as_slice()),
            Self::V8 => (chunks::ArchiveVersion::V8, &["8", "80"].as_slice()),
        };
        TargetDescriptor {
            id: dialect.id(),
            aliases,
        }
    }

    const fn value(self) -> u64 {
        match self {
            Self::V5 => 50,
            Self::V6 => 60,
            Self::V7 => 70,
            Self::V8 => 80,
        }
    }

    const fn uses_extended_brep_layout(self) -> bool {
        !matches!(self, Self::V5)
    }

    const fn uses_face_array_v2(self) -> bool {
        matches!(self, Self::V7 | Self::V8)
    }

    const fn stores_mesh_vertices_as_f64(self) -> bool {
        !matches!(self, Self::V5)
    }
}

impl CodecBackend for RhinoCodec {
    const FORMAT: &'static str = dialect::FORMAT;

    fn detect_impl(&self, prefix: &[u8]) -> Confidence {
        if prefix.windows(MAGIC.len()).any(|window| window == MAGIC) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        container::inspect(root)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        container::decode(ctx, root)
    }
}

impl EncoderBackend for RhinoCodec {
    const FORMAT: &'static str = dialect::FORMAT;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(RhinoArchiveVersion::TARGETS, Some(3));

    /// Synthesis-only encoder. An off-catalog Rhino source cannot be reproduced
    /// because 3DM has no retained-image path.
    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        writer::target::plan(input, &target)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
