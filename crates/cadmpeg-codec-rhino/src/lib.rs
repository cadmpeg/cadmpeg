// SPDX-License-Identifier: Apache-2.0
//! Reads and writes Rhino `.3dm` files through [`cadmpeg_ir::document::CadIr`].
//!
//! Support level: L8 for archive versions 50, 60, 70, and 80 on the cadmpeg
//! support ladder. The codec provides bounded 3DM container inspection, typed
//! decoding, and explicitly versioned semantic native writing.

use crate::loss::RhinoLossCode;
use cadmpeg_ir::codec::{Codec, CodecError, Confidence, ContainerSummary, DecodeResult, Encoder};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::ExportReport;
use std::io::Write;

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
pub(crate) mod loss;
pub(crate) mod mesh;
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

#[cfg(feature = "fuzzing")]
pub mod fuzzing;

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

impl Codec for RhinoCodec {
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
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        container::inspect(ctx, root)
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

    fn encode(&self, ir: &CadIr, output: &mut dyn Write) -> Result<ExportReport, CodecError> {
        writer::write(ir, self.version.value(), output)?;
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        let vertex_quantization = self.version == RhinoArchiveVersion::V5
            && ir
                .model
                .tessellations
                .iter()
                .flat_map(|mesh| &mesh.vertices)
                .any(|point| {
                    f64::from(point.x as f32) != point.x
                        || f64::from(point.y as f32) != point.y
                        || f64::from(point.z as f32) != point.z
                });
        let normal_quantization = ir
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
                RhinoLossCode::MeshVertexQuantized
                    .note("archive version 50 stores standalone mesh vertices as f32"),
            );
        }
        if normal_quantization {
            losses.push(
                RhinoLossCode::MeshNormalQuantized.note("3DM mesh normals are stored as f32"),
            );
        }
        Ok(ExportReport {
            format: "rhino".into(),
            total_entities,
            entity_counts: validation.entity_counts,
            losses,
            notes: vec![format!("3DM archive version {}", self.version.value())],
        })
    }
}

impl Encoder for RhinoCodec {
    fn id(&self) -> &'static str {
        "rhino"
    }

    fn encode(&self, ir: &CadIr, output: &mut dyn Write) -> Result<ExportReport, CodecError> {
        RhinoEncoder::new(RhinoArchiveVersion::V8).encode(ir, output)
    }
}

#[cfg(test)]
mod archive_test_support;
#[cfg(test)]
mod archive_tests;
#[cfg(test)]
mod tests;
