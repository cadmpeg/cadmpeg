// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Decode bare Autodesk `ShapeManager` (ASM) B-rep streams.
//!
//! A bare stream is an ASM serialization outside any container: a binary
//! `.smb`/`.smbh`-style SAB stream or a text `.sat`/`.smt` stream. Content
//! selects the path, never the file extension: the `ASM BinaryFile` magic
//! selects the binary framer and the ASCII header lines select the text
//! parser. Both paths decode through the shared kernel decoders in
//! [`cadmpeg_asm::brep`] into the neutral model arenas, with the kernel-side
//! native records under the `sat` namespace.
//!
//! A Spatial ACIS binary stream (`ACIS BinaryFile` magic) uses version-gated
//! layouts the ASM decoders do not cover; it is identified and reported
//! without a decode attempt. A text stream frames on either branch
//! terminator, and its decode outcome — not the terminator — decides whether
//! the report carries geometry or an unsupported-branch finding.

use cadmpeg_asm::asm_header;
use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::{sab, sat};
use cadmpeg_codec_core::decode::{DecodeContext, View};
use cadmpeg_codec_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeResult};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::{DecodeReport, LossKind, LossNote, Severity};
use cadmpeg_ir::units::{Tolerances, Units};
use std::collections::BTreeMap;

/// The stable format identifier and native namespace.
const FORMAT: &str = "sat";

/// The Spatial ACIS binary magic prefix. The ASM decoders do not cover the
/// version-gated ACIS binary layouts, so the stream is identified and
/// reported without a decode attempt.
const ACIS_BINARY_MAGIC: &[u8] = b"ACIS BinaryFile";

/// The stream encoding a byte prefix selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    /// `ASM BinaryFile4`/`ASM BinaryFile8` SAB.
    AsmBinary,
    /// `ACIS BinaryFile` SAB (identified, not decoded).
    AcisBinary,
    /// Text header lines.
    Text,
    /// Not an ASM stream.
    Unknown,
}

fn classify(prefix: &[u8]) -> StreamKind {
    if asm_header::has_asm_magic(prefix) {
        return StreamKind::AsmBinary;
    }
    if prefix.starts_with(ACIS_BINARY_MAGIC) {
        return StreamKind::AcisBinary;
    }
    if looks_like_text_stream(prefix) {
        return StreamKind::Text;
    }
    StreamKind::Unknown
}

/// Whether the prefix opens like a text stream: a first line of four ASCII
/// integer fields (the four header words) followed by a counted-string line.
fn looks_like_text_stream(prefix: &[u8]) -> bool {
    if !sat::has_text_magic(prefix) {
        return false;
    }
    let Some(line_end) = prefix.iter().position(|byte| *byte == b'\n') else {
        return false;
    };
    let fields: Vec<&[u8]> = prefix[..line_end]
        .split(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        .filter(|field| !field.is_empty())
        .collect();
    fields.len() == 4
        && fields
            .iter()
            .all(|field| std::str::from_utf8(field).is_ok_and(|field| field.parse::<i64>().is_ok()))
        && prefix.get(line_end + 1).is_some_and(u8::is_ascii_digit)
}

/// Bare ASM stream codec.
pub struct SatCodec;

impl Codec for SatCodec {
    fn id(&self) -> &'static str {
        FORMAT
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        match classify(prefix) {
            StreamKind::AsmBinary | StreamKind::AcisBinary => Confidence::High,
            // The text opening is a weak signature shared with other numeric
            // text files, so detection defers to stronger magics.
            StreamKind::Text => Confidence::Medium,
            StreamKind::Unknown => Confidence::No,
        }
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let bytes = root.window();
        let mut attributes = BTreeMap::new();
        let mut notes = Vec::new();
        let kind = classify(bytes);
        match kind {
            StreamKind::AsmBinary => {
                if let Some(header) = asm_header::parse(bytes) {
                    header_attributes(&header, &mut attributes);
                    if header.has_history_partition() {
                        notes.push(
                            "the stream declares a construction-history partition; decode reads \
                             the solved partition"
                                .to_string(),
                        );
                    }
                }
            }
            StreamKind::AcisBinary => {
                notes.push(
                    "Spatial ACIS binary stream: identified, and its version-gated layouts are \
                     not decoded"
                        .to_string(),
                );
            }
            StreamKind::Text => match sat::parse(bytes) {
                Ok(stream) => {
                    header_attributes(&stream.header.as_asm_header(), &mut attributes);
                    attributes.insert("scale".to_string(), format!("{}", stream.header.scale));
                    attributes.insert("records".to_string(), stream.records.len().to_string());
                    attributes.insert(
                        "terminator".to_string(),
                        match stream.dialect {
                            sat::Dialect::Asm => "End-of-ASM-data",
                            sat::Dialect::Acis => "End-of-ACIS-data",
                        }
                        .to_string(),
                    );
                }
                Err(error) => notes.push(format!("text stream does not parse: {error}")),
            },
            StreamKind::Unknown => {
                return Err(CodecError::Malformed(
                    "not an ASM stream: no binary magic and no text header lines".to_string(),
                ))
            }
        }
        Ok(ContainerSummary {
            format: FORMAT.to_string(),
            container_kind: "stream".to_string(),
            entries: vec![ContainerEntry {
                name: "stream".to_string(),
                role: match kind {
                    StreamKind::AsmBinary => "brep",
                    StreamKind::AcisBinary => "acis-binary",
                    StreamKind::Text => "brep-text",
                    StreamKind::Unknown => unreachable!("unknown kind returned above"),
                }
                .to_string(),
                compression: "stored".to_string(),
                compressed_size: bytes.len() as u64,
                uncompressed_size: bytes.len() as u64,
                attributes,
            }],
            notes,
        })
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let bytes = root.window();
        match classify(bytes) {
            StreamKind::AsmBinary => decode_binary(ctx, bytes),
            StreamKind::Text => decode_text(ctx, bytes),
            StreamKind::AcisBinary => Ok(unsupported_result(
                "Spatial ACIS binary stream: the ASM decoders do not cover its version-gated \
                 layouts, so the stream is identified and not decoded",
            )),
            StreamKind::Unknown => Err(CodecError::Malformed(
                "not an ASM stream: no binary magic and no text header lines".to_string(),
            )),
        }
    }
}

fn header_attributes(header: &asm_header::AsmHeader, attributes: &mut BTreeMap<String, String>) {
    if let Some(version) = header.save_format_version {
        attributes.insert("acis_save_format_version".to_string(), version.to_string());
    }
    if let Some(count) = header.entity_count {
        attributes.insert("asm_entity_count".to_string(), count.to_string());
    }
    if let Some(flags) = header.flags {
        attributes.insert("asm_flags".to_string(), flags.to_string());
    }
    if let Some(family) = &header.product_family {
        attributes.insert("product_family".to_string(), family.clone());
    }
    if let Some(version) = &header.product_version {
        attributes.insert("product_version".to_string(), version.clone());
    }
    if let Some(date) = &header.save_date {
        attributes.insert("save_date".to_string(), date.clone());
    }
}

fn decode_binary(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    let header = asm_header::parse(bytes).ok_or_else(|| {
        CodecError::Malformed("ASM binary magic without a parseable header".to_string())
    })?;
    let width = usize::from(header.width);
    let start = asm_header::record_stream_start(bytes).ok_or_else(|| {
        CodecError::Malformed("ASM binary header without a record stream".to_string())
    })?;
    // A history-bearing stream ends its solved partition at the delta-state
    // boundary; a history-less stream ends at EOF without a terminator tag.
    let framed = match asm_header::solved_record_limit(bytes) {
        Some(limit) => sab::frame(bytes, start, limit, width),
        None => sab::frame_history(bytes, start, bytes.len(), width),
    };
    let records =
        framed.map_err(|error| CodecError::Malformed(format!("SAB framing failed: {error}")))?;
    let brep = decode_with_header(
        &records,
        bytes,
        Some(header.clone()),
        "stream",
        IdFormat(FORMAT),
        DecodePurpose::Model,
    );
    let mut attributes = BTreeMap::new();
    header_attributes(&header, &mut attributes);
    attributes.insert("encoding".to_string(), "binary".to_string());
    build_result(ctx, brep, attributes, &header, None)
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    let stream = sat::parse(bytes)
        .map_err(|error| CodecError::Malformed(format!("text stream parse failed: {error}")))?;
    let header = stream.header.as_asm_header();
    let brep = decode_with_header(
        &stream.records,
        bytes,
        Some(header.clone()),
        "stream",
        IdFormat(FORMAT),
        DecodePurpose::Model,
    );
    let mut attributes = BTreeMap::new();
    header_attributes(&header, &mut attributes);
    attributes.insert("encoding".to_string(), "text".to_string());
    attributes.insert("scale".to_string(), format!("{}", stream.header.scale));
    let dialect = match stream.dialect {
        sat::Dialect::Asm => "End-of-ASM-data",
        sat::Dialect::Acis => "End-of-ACIS-data",
    };
    attributes.insert("terminator".to_string(), dialect.to_string());
    build_result(ctx, brep, attributes, &header, Some(dialect))
}

fn build_result(
    ctx: &DecodeContext<'_>,
    brep: AsmBrep,
    attributes: BTreeMap<String, String>,
    header: &asm_header::AsmHeader,
    text_dialect: Option<&str>,
) -> Result<DecodeResult, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(SourceMeta {
        format: FORMAT.to_string(),
        attributes,
    });
    if let (Some(linear), Some(angular)) = (header.linear, header.angular) {
        ir.tolerances = Tolerances { linear, angular };
    }

    ir.model.bodies = brep.bodies;
    ir.model.regions = brep.regions;
    ir.model.shells = brep.shells;
    ir.model.faces = brep.faces;
    ir.model.loops = brep.loops;
    ir.model.coedges = brep.coedges;
    ir.model.edges = brep.edges;
    ir.model.vertices = brep.vertices;
    ir.model.points = brep.points;
    ir.model.surfaces = brep.surfaces;
    ir.model.curves = brep.curves;
    ir.model.pcurves = brep.pcurves;
    ir.model.procedural_surfaces = brep.procedural_surfaces;
    ir.model.procedural_curves = brep.procedural_curves;
    ir.model.attributes = brep.attributes;
    ctx.charge_entities(ir.model.entity_count() as u64, "admit ASM entities")?;

    let namespace = ir.native.namespace_mut(FORMAT);
    namespace.version = 1;
    let store = |error: cadmpeg_ir::NativeConvertError| {
        CodecError::Malformed(format!("native arena serialization failed: {error}"))
    };
    namespace
        .set_arena("edge_continuities", &brep.edge_continuities)
        .map_err(store)?;
    namespace
        .set_arena("edge_ownerships", &brep.edge_ownerships)
        .map_err(store)?;
    namespace
        .set_arena("vertex_ownerships", &brep.vertex_ownerships)
        .map_err(store)?;
    namespace
        .set_arena("face_sidedness", &brep.face_sidedness)
        .map_err(store)?;
    namespace
        .set_arena("tolerant_vertex_tails", &brep.tolerant_vertex_tails)
        .map_err(store)?;
    namespace
        .set_arena("tolerant_edge_tails", &brep.tolerant_edge_tails)
        .map_err(store)?;
    namespace
        .set_arena(
            "tolerant_coedge_parameters",
            &brep.tolerant_coedge_parameters,
        )
        .map_err(store)?;
    namespace
        .set_arena("mesh_surface_sentinels", &brep.mesh_surface_sentinels)
        .map_err(store)?;
    namespace
        .set_arena("wire_topologies", &brep.wire_topologies)
        .map_err(store)?;
    namespace
        .set_arena("transform_hints", &brep.transform_hints)
        .map_err(store)?;
    namespace
        .set_arena("body_native_keys", &brep.body_native_keys)
        .map_err(store)?;

    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
    let mut losses = Vec::new();
    if !geometry_transferred {
        let branch = text_dialect.map_or(String::new(), |dialect| {
            format!(" The stream ends with `{dialect}`.")
        });
        losses.push(LossNote {
            code: LossKind::GeometryNotTransferred,
            severity: Severity::Blocking,
            message: format!(
                "the stream framed but its records decoded no surfaces, points, or faces; its \
                 version or branch is outside the ASM decoders' coverage.{branch}"
            ),
            provenance: None,
        });
    }
    if brep.stats.unknown_surface_faces > 0 {
        losses.push(LossNote {
            code: LossKind::GeometryNotTransferred,
            severity: Severity::Warning,
            message: format!(
                "{} face(s) rest on procedural surface constructions without a decoded carrier",
                brep.stats.unknown_surface_faces
            ),
            provenance: None,
        });
    }
    let mut coverage = BTreeMap::new();
    coverage.insert("unknown_records".to_string(), brep.unknowns.len());
    coverage.insert(
        "unknown_surface_faces".to_string(),
        brep.stats.unknown_surface_faces,
    );
    let report = DecodeReport {
        format: FORMAT.to_string(),
        container_only: false,
        geometry_transferred,
        coverage,
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: Vec::new(),
    };

    let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
    source_fidelity
        .attach_native_unknown_records(&mut ir, FORMAT, brep.unknowns)
        .map_err(|error| {
            CodecError::Malformed(format!("unknown-record retention failed: {error}"))
        })?;
    Ok(DecodeResult::new(ir, report, source_fidelity))
}

/// A result for an identified stream the decoders do not cover.
fn unsupported_result(message: &str) -> DecodeResult {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(SourceMeta {
        format: FORMAT.to_string(),
        attributes: BTreeMap::new(),
    });
    let report = DecodeReport {
        format: FORMAT.to_string(),
        container_only: false,
        geometry_transferred: false,
        coverage: BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: vec![LossNote {
            code: LossKind::GeometryNotTransferred,
            severity: Severity::Blocking,
            message: message.to_string(),
            provenance: None,
        }],
        notes: Vec::new(),
    };
    DecodeResult::new(ir, report, cadmpeg_ir::SourceFidelity::default())
}

#[cfg(test)]
mod tests;
