// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes Rhino `.3dm` files through [`cadmpeg_ir::document::CadIr`].
//!
//! <!-- generated: capability -->
//! Support: depth L1, breadth 6 of 8 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#rhino-3dm)).
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

/// What resolving a [`TargetRequest`] against the source decided (design §8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resolution {
    /// The archive version to write.
    version: RhinoArchiveVersion,
    /// Why the source's own dialect is not what gets written, when it is not.
    /// `None` where the write keeps the source's dialect, and `None` where
    /// there is no Rhino source at all: nothing was preserved, so nothing was
    /// lost.
    declined: Option<String>,
}

impl RhinoEncoder {
    /// Resolve the request against the source into the archive version to
    /// write.
    ///
    /// `Explicit(id)` refuses an id outside the synthesis catalog and otherwise
    /// names its archive version outright. Where that version is not the
    /// source's own dialect the write changes what the file is, so the
    /// resolution declines preservation by name and the report charges the
    /// fidelity: the caller asked for archive 80 and the archive 50 identity
    /// the source carried is gone.
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
    /// A Rhino source that records no dialect is refused too: there is nothing
    /// to preserve, and no identity to default to.
    ///
    /// The catalog default supplies the target only when there is nothing to
    /// inherit: the document has no source, or a source of another format. That
    /// is the cross-format path, where the application layer would have built
    /// `Explicit(catalog default)` itself. The encoder holds no version of its
    /// own; an encoder-held one used to override every other answer, which is
    /// how an archive-50 source came back as archive 80.
    fn resolve(
        ir: &cadmpeg_ir::document::CadIr,
        request: TargetRequest<'_>,
    ) -> Result<Resolution, CodecError> {
        let id = match request {
            TargetRequest::Explicit(id) => id,
            TargetRequest::Inherit => {
                return match cadmpeg_ir::codec::resolve_inherit(
                    ir,
                    dialect::FORMAT,
                    dialect::TARGETS,
                )? {
                    cadmpeg_ir::codec::Inherited::Fallback(id) => Ok(Resolution {
                        version: dialect::target_version(id)
                            .expect("the Rhino catalog default is a synthesis target"),
                        declined: None,
                    }),
                    cadmpeg_ir::codec::Inherited::Source(source_dialect) => {
                        dialect::target_version(source_dialect.as_str())
                            .map(|version| Resolution {
                                version,
                                declined: None,
                            })
                            .ok_or_else(|| {
                                cadmpeg_ir::codec::unsupported_target(
                                    dialect::FORMAT,
                                    source_dialect.as_str(),
                                    "the source archive version is one this writer cannot \
                                     synthesize, and 3DM has no byte-replay path that could \
                                     preserve it",
                                    dialect::TARGETS,
                                )
                            })
                    }
                };
            }
        };
        let version = dialect::target_version(id).ok_or_else(|| {
            cadmpeg_ir::codec::unsupported_target(
                dialect::FORMAT,
                id,
                "not a target this encoder can synthesize",
                dialect::TARGETS,
            )
        })?;
        let target = version.target();
        let declined = ir
            .source
            .as_ref()
            .filter(|source| source.format == dialect::FORMAT)
            .and_then(|source| source.dialect.as_ref())
            .filter(|source_dialect| source_dialect.as_str() != target)
            .map(|source_dialect| {
                format!(
                    "source is {source_dialect}, target is {target}; the source archive version is \
                     not what this export writes"
                )
            });
        Ok(Resolution { version, declined })
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
        let Resolution { version, declined } = Self::resolve(input.ir, request)?;
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
        let report = ExportReport {
            target: Some(cadmpeg_core::dialect::DialectId::pinned(version.target())),
            format: "rhino".into(),
            census: cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            // A write that changes the source's archive version charges the
            // fidelity, naming both dialects. It is not "fidelity was never
            // offered": an identity the source carried is gone, and the report
            // is where that is stated.
            fidelity: match declined {
                Some(reason) => FidelityResolution::Degraded { reason },
                None if input.fidelity.is_some() => FidelityResolution::NotConsumed,
                None => FidelityResolution::NotProvided,
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
