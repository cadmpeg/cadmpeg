// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes Rhino `.3dm` files through [`cadmpeg_ir::document::CadIr`].
//!
//! Support level: [L1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder) for the archive 2/3/4/50/60/70/80/90 chunked band.
//! Archive 2/3/4/50/60/70/80/90 and V2–V4 open at L1 and show as extras.
//! V1 and archive version 5 remain L0. The codec provides bounded 3DM
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
        match self {
            Self::V5 => "rhino:archive-50",
            Self::V6 => "rhino:archive-60",
            Self::V7 => "rhino:archive-70",
            Self::V8 => "rhino:archive-80",
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
}

/// Native 3DM encoder with an explicit target archive version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhinoEncoder {
    version: RhinoArchiveVersion,
}

impl RhinoEncoder {
    /// Select a target archive version.
    ///
    /// The version is the fallback for a request with nothing to inherit, not
    /// the encoder's answer to every request: `TargetRequest` decides what gets
    /// written, and this version is consulted only when a request has nothing
    /// to inherit.
    pub const fn new(version: RhinoArchiveVersion) -> Self {
        Self { version }
    }
}

impl Default for RhinoEncoder {
    /// The catalog default, [`RhinoArchiveVersion::V8`] — the `default: true`
    /// row of [`Encoder::targets`], and the same version a cross-format
    /// conversion into 3DM resolves to.
    fn default() -> Self {
        Self::new(RhinoArchiveVersion::V8)
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

impl RhinoEncoder {
    /// Resolve the request against the source into the archive version to
    /// write.
    ///
    /// `Explicit(id)` refuses an id outside the synthesis catalog and otherwise
    /// names its archive version outright.
    ///
    /// `Inherit` asks for preservation: the source's own dialect, synthesized.
    /// The replay law is inapplicable here — the 3DM writer builds every chunk
    /// from the neutral IR and has no retained-source branch, so synthesis is
    /// the only write path this codec has and `id == source.dialect` is never a
    /// question about bytes. That makes preservation strictly narrower than in a
    /// replaying codec: a source dialect outside the catalog cannot be written
    /// back at all, so `Inherit` refuses it, naming the source dialect and the
    /// catalog. That band is real — archives 2, 3, 4 and 90 decode without a
    /// writer, and 1, 5 and unknown words do not decode — and an explicit
    /// `--rhino-target` is the escape. There is no fall-through to the catalog
    /// default: a same-format conversion never silently changes what the file
    /// is, which is exactly the archive-50 source that used to come back as
    /// archive 80.
    ///
    /// `self.version` supplies the target only when there is nothing to
    /// inherit: the document is not a Rhino document, or it records no dialect.
    /// Neither case is reachable from the command line, which builds `Inherit`
    /// only for a Rhino source.
    fn resolve(
        self,
        ir: &cadmpeg_ir::document::CadIr,
        request: TargetRequest<'_>,
    ) -> Result<RhinoArchiveVersion, CodecError> {
        match request {
            TargetRequest::Explicit(id) => dialect::target_version(id).ok_or_else(|| {
                cadmpeg_ir::codec::unsupported_target(
                    dialect::FORMAT,
                    id,
                    "not a target this encoder can synthesize",
                    dialect::TARGETS,
                )
            }),
            TargetRequest::Inherit => {
                let Some(source_dialect) = ir
                    .source
                    .as_ref()
                    .filter(|source| source.format == dialect::FORMAT)
                    .and_then(|source| source.dialect.as_ref())
                else {
                    // Nothing to inherit: no Rhino source, or one that records
                    // no dialect.
                    return Ok(self.version);
                };
                dialect::target_version(source_dialect.as_str()).ok_or_else(|| {
                    cadmpeg_ir::codec::unsupported_target(
                        dialect::FORMAT,
                        source_dialect.as_str(),
                        "the source archive version is one this writer cannot synthesize, and 3DM \
                         has no byte-replay path that could preserve it",
                        dialect::TARGETS,
                    )
                })
            }
        }
    }
}

impl Encoder for RhinoEncoder {
    fn id(&self) -> &'static str {
        "rhino"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        dialect::TARGETS
    }

    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        let version = self.resolve(input.ir, request)?;
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
            target: Some(cadmpeg_core::dialect::DialectId::pinned(version.target())),
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
            notes: vec![format!("3DM archive version {}", version.value())],
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
