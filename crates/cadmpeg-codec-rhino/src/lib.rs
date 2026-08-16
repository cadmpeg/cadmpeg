// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes Rhino `.3dm` files through [`cadmpeg_ir::document::CadIr`].
//!
//! Support level: L1 for archive versions 2, 3, 4, 50, 60, 70, and 80; L0 for
//! V1 and archive version 5 on the cadmpeg support ladder. The codec provides
//! bounded 3DM container inspection, partial typed decoding, and explicitly
//! versioned semantic native writing.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{CodecBackend, Confidence, DecodeResult, EncodeInput, Encoder, ExportPlan};
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
    const fn value(self) -> u64 {
        match self {
            Self::V5 => 50,
            Self::V6 => 60,
            Self::V7 => 70,
            Self::V8 => 80,
        }
    }
}

/// Native 3DM encoder with an explicit target archive version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhinoEncoder {
    version: RhinoArchiveVersion,
}

impl RhinoEncoder {
    /// Select a target archive version.
    pub const fn new(version: RhinoArchiveVersion) -> Self {
        Self { version }
    }
}

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

impl Encoder for RhinoEncoder {
    fn id(&self) -> &'static str {
        "rhino"
    }

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        let mut bytes = Vec::new();
        writer::write(input.ir, self.version.value(), &mut bytes)?;
        let vertex_quantization = self.version == RhinoArchiveVersion::V5
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
        if vertex_quantization {
            losses.push(
                loss::RhinoLossCode::MeshVertexPrecisionReduced
                    .note("archive version 50 stores standalone mesh vertices as f32"),
            );
        }
        if normal_quantization {
            losses.push(
                loss::RhinoLossCode::MeshNormalPrecisionReduced
                    .note("3DM mesh normals are stored as f32"),
            );
        }
        let report = ExportReport {
            format: "rhino".into(),
            census: cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            fidelity: if input.fidelity.is_some() {
                FidelityResolution::NotConsumed
            } else {
                FidelityResolution::NotProvided
            },
            // The 3DM writer builds every chunk from the neutral IR; it has no
            // retained-source branch.
            write_path: WritePath::Synthesized,
            losses,
            notes: vec![format!("3DM archive version {}", self.version.value())],
        };
        Ok(ExportPlan::buffered(report, bytes))
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
