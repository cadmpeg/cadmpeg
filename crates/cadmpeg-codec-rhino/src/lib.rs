// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes Rhino `.3dm` files through [`cadmpeg_ir::document::CadIr`].
//!
//! <!-- generated: capability -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#rhino-3dm)).
//! <!-- /generated: capability -->
//!
//! The codec provides bounded 3DM
//! container inspection, partial typed decoding, and explicitly versioned
//! semantic native writing.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeResult, EncodeInput, Encoder, ExportPlan, TargetDescriptor,
    TargetRequest,
};
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{FidelityResolution, WritePath};

pub(crate) mod annotations;
pub(crate) mod brep;
pub(crate) mod cage;
pub(crate) mod chunks;
pub(crate) mod container;
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

impl RhinoArchiveVersion {
    /// The registry dialect id this version writes.
    ///
    /// The spelling a caller passes as `TargetRequest::Explicit`.
    #[must_use]
    pub const fn target(self) -> &'static str {
        self.pinned()
    }

    const fn value(self) -> u64 {
        match self {
            Self::V5 => 50,
            Self::V6 => 60,
            Self::V7 => 70,
            Self::V8 => 80,
        }
    }
}

/// Native 3DM encoder.
///
/// Carries no target state. Which archive version an export writes is
/// [`TargetRequest`]'s answer, resolved against the source: an explicit target
/// names it, `Inherit` synthesizes the source's own, and a document with
/// nothing to inherit falls to the catalog default — the `default: true` row of
/// [`Encoder::targets`], which is also what a cross-format conversion into 3DM
/// resolves to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RhinoEncoder;

impl CodecBackend for RhinoCodec {
    fn id(&self) -> &'static str {
        "rhino"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
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

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        container::decode(ctx, root, ctx.container_only())
    }
}

/// Why this writer cannot reproduce a source archive version outside
/// [`dialect::TARGETS`].
///
/// The one per-codec sentence of the shared catalog-write resolution. The band
/// is real — archives 1, 2, 3, 4, 5 and 90 decode without a writer, archive 5
/// and unknown words decode as admitted-unverified, and 3DM has no retained-
/// image path that could write any of them back.
const OFF_CATALOG_SOURCE_REASON: &str =
    "the source archive version is one this writer cannot synthesize, and 3DM has no byte-replay \
     path that could preserve it";

impl Encoder for RhinoEncoder {
    fn id(&self) -> &'static str {
        "rhino"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        dialect::TARGETS
    }

    /// Synthesis-only encoder; resolution is owned by
    /// [`cadmpeg_ir::codec::resolve_catalog_write`]. An off-catalog Rhino source
    /// cannot be reproduced because 3DM has no retained-image path.
    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        let (entry, displaced) = cadmpeg_ir::codec::resolve_catalog_write(
            input.ir,
            request,
            dialect::FORMAT,
            dialect::TARGETS,
            OFF_CATALOG_SOURCE_REASON,
        )?;
        let version = dialect::target_version(entry);
        let mut bytes = Vec::new();
        writer::write(input.ir, version.value(), &mut bytes)?;
        let vertex_quantization = version == RhinoArchiveVersion::V5
            && input
                .ir
                .model
                .tessellations
                .iter()
                .flat_map(|mesh| &mesh.vertices)
                .any(|point| {
                    f64::from(point.x as f32) != point.x
                        || f64::from(point.y as f32) != point.y
                        || f64::from(point.z as f32) != point.z
                });
        let normal_quantization = input
            .ir
            .model
            .tessellations
            .iter()
            .flat_map(|mesh| &mesh.normals)
            .any(|normal| {
                f64::from(normal.x as f32) != normal.x
                    || f64::from(normal.y as f32) != normal.y
                    || f64::from(normal.z as f32) != normal.z
            });
        let mut losses = Vec::new();
        let target = cadmpeg_core::dialect::DialectId::pinned(version.target());
        if let Some(source) = displaced.as_ref() {
            losses.push(loss::RhinoLossCode::SourceDialectDisplaced.note(
                cadmpeg_ir::codec::source_dialect_displaced_message(source, &target),
            ));
        }
        if vertex_quantization {
            losses.push(loss::RhinoLossCode::MeshVertexPrecisionReduced.note(
                "archive version 50 stores standalone mesh vertices as f32; \
                 rhino:archive-60, rhino:archive-70, and rhino:archive-80 store them as f64 \
                 and would not charge this",
            ));
        }
        if normal_quantization {
            losses.push(loss::RhinoLossCode::MeshNormalPrecisionReduced.note(
                "3DM mesh normals are stored as f32; every rhino write target charges this, \
                 so no other target avoids it",
            ));
        }
        let report = ExportReport::native(
            target,
            "rhino".into(),
            cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            // Dialect displacement is an export loss. Fidelity states only
            // whether the optional sidecar was consumed by this writer.
            if input.fidelity.is_some() {
                FidelityResolution::NotConsumed
            } else {
                FidelityResolution::NotProvided
            },
            // The 3DM writer builds every chunk from the neutral IR; it has no
            // retained-source branch.
            WritePath::Synthesized,
            losses,
            vec![format!("3DM archive version {}", version.value())],
        );
        Ok(ExportPlan::buffered(report, bytes))
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
