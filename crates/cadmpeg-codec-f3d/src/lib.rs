// SPDX-License-Identifier: Apache-2.0
//! Read and write Autodesk Fusion `.f3d` archives.
//!
//! [`F3dCodec`] implements [`Codec`] and [`Encoder`]. Decoding produces a
//! [`CadIr`] document with B-rep topology, analytic and cached NURBS geometry,
//! body transforms, design and sketch records, construction history, and
//! appearances. Encoding replays an unchanged decoded archive byte for byte,
//! applies supported semantic edits to retained source data, or creates an
//! archive from the supported source-less profile.
//!
//! Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Decode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! for loss in &result.report.losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`] classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeOptions::container_only` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let mut result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! // Edit supported fields in result.ir.
//! let mut output = File::create("part-edited.f3d")?;
//! F3dCodec.encode(&result.ir, &mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Data flow
//!
//! [`container`] selects the authoritative `.smbh` B-rep, or the first `.smb`
//! construction snapshot when no `.smbh` exists. [`sab`] frames its active
//! record slice. [`brep`] builds the topology chain from bodies through
//! vertices and points, while [`nurbs`] decodes cached spline carriers.
//! [`design`], [`history`], and [`materials`] populate source-native records and
//! appearance bindings.
//!
//! ASM model-space lengths become millimetres. Directions, ratios, angles,
//! knots, weights, and UV parameters retain their native scale.
//!
//! Inspect [`cadmpeg_ir::report::DecodeReport::losses`] before consuming a
//! decode. A stream that cannot produce geometry returns container metadata,
//! retained source data, and blocking geometry and topology losses. Referenced
//! carrier bytes needed for passthrough remain available as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.

mod act;
pub mod asm_header;
pub mod brep;
pub mod container;
pub mod decode;
pub mod design;
pub mod history;
mod history_records;
pub mod materials;
mod native;
pub mod nurbs;
pub mod records;
pub mod sab;
mod writer;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Check, Finding, Severity};
use std::io::Write;

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

/// Validate Fusion native design-record relationships and exact sketch frames.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    use std::collections::HashSet;

    let Some(namespace) = ir.native.namespace("f3d") else {
        return Vec::new();
    };
    if namespace.version != native::F3D_NATIVE_VERSION {
        let version = namespace.version;
        return vec![Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!("unsupported Fusion native namespace version {version}"),
            entity: None,
        }];
    }
    let Ok(native) = native::F3dNative::load(namespace) else {
        return vec![Finding {
            check: Check::NativeLinks,
            severity: Severity::Error,
            message: "Fusion native namespace does not match schema version 1".into(),
            entity: None,
        }];
    };
    let mut findings = Vec::new();
    let record_indices = native
        .design_record_headers
        .iter()
        .map(|record| record.record_index)
        .collect::<HashSet<_>>();
    for header in &native.design_entity_headers {
        let count_matches = header
            .declared_reference_count
            .is_none_or(|count| count as usize == header.reference_indices.len());
        let references_resolve = header
            .reference_indices
            .iter()
            .all(|index| record_indices.contains(index));
        if !count_matches || !references_resolve {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "Fusion design entity has an invalid reference run".into(),
                entity: Some(header.entity_id.clone()),
            });
        }
    }
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
        .map(|header| header.entity_suffix as u32)
        .collect::<HashSet<_>>();
    for relation in &native.sketch_relations {
        const CONSTRAINT_MASK: u32 = 0x3000_3ff7;
        let valid = sketch_owners.contains(&relation.owner_reference)
            && relation.raw_bytes.len() == 101
            && relation.unknown_constraint_bits == relation.state & !CONSTRAINT_MASK
            && relation.constraint_kinds.len()
                == (relation.state & CONSTRAINT_MASK).count_ones() as usize;
        if !valid {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "Fusion sketch relation has an invalid owner or byte frame".into(),
                entity: Some(relation.id.clone()),
            });
        }
    }
    for point in &native.sketch_points {
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "Fusion sketch point contains a non-finite coordinate".into(),
                entity: Some(point.id.clone()),
            });
        }
    }
    findings
}

impl F3dCodec {
    /// Write a decoded F3D document using its source-fidelity sidecar.
    pub fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &cadmpeg_ir::SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let record = source_fidelity
            .retained_record("f3d:file:source-image#0")
            .ok_or_else(|| {
                CodecError::NotImplemented("sidecar has no retained F3D source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained F3D source image has no bytes".into())
        })?;
        Self::write_preserved_bytes(ir, data, record.byte_len, &record.sha256, writer)
    }

    fn write_preserved_bytes(
        ir: &CadIr,
        data: &[u8],
        byte_len: u64,
        sha256: &str,
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"))
            .ok_or_else(|| CodecError::NotImplemented("IR has no F3D semantic baseline".into()))?;
        let hash = sha256_hex(data);
        if data.len() as u64 != byte_len || hash != sha256 {
            return Err(CodecError::Malformed(
                "retained F3D source image failed integrity validation".into(),
            ));
        }
        if decode::semantic_hash(ir) != *expected {
            return writer::write_semantic(ir, data, writer);
        }
        writer.write_all(data)?;
        Ok(())
    }
}

impl Codec for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(ZIP_MAGIC) {
            return Confidence::No;
        }
        // A ZIP alone is a weak signal (many formats are ZIPs). An f3d marker
        // string in the prefix — entry names are stored in cleartext in ZIP
        // local headers — makes it conclusive.
        if container::DETECT_MARKERS
            .iter()
            .any(|m| contains_subslice(prefix, m))
        {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(reader)?;
        Ok(container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(reader, options)
    }
}

impl Encoder for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        writer::write_new(ir, writer)?;
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "f3d".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: vec![
                "source container regenerated from IR".into(),
                "entity counts are derived from the IR".into(),
            ],
        })
    }

    fn encode_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: Option<&cadmpeg_ir::SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let replay = source_fidelity
            .and_then(|sidecar| sidecar.retained_record("f3d:file:source-image#0"))
            .is_some();
        if let Some(sidecar) = source_fidelity.filter(|_| replay) {
            self.write_preserved_with_source_fidelity(ir, sidecar, writer)?;
        } else {
            writer::write_new(ir, writer)?;
        }
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "f3d".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: vec![
                if replay {
                    "preserved source container replayed verbatim"
                } else {
                    "source container regenerated from IR"
                }
                .into(),
                "entity counts are derived from the IR".into(),
            ],
        })
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests;
