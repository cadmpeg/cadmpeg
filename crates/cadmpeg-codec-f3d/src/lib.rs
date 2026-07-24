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
//! use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
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
//! [`CodecEntry::inspect`](cadmpeg_ir::codec::CodecEntry::inspect) classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeOptions::container_only` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions, Encoder};
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
//! [`container`] selects the `.smbh` history stream, or the first `.smb` when
//! no `.smbh` exists. [`sab`] frames its active record slice. The Design body
//! map selects every B-rep blob contributing bodies to the document model;
//! [`brep`] builds each topology chain from bodies through vertices and points,
//! while [`nurbs`] decodes cached spline carriers.
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

#![allow(clippy::disallowed_methods)]

mod act;
pub mod asm_header;
pub mod brep;
mod bytes;
pub mod container;
pub mod decode;
pub mod design;
pub mod f3z;
pub mod history;
mod history_records;
mod ids;
pub mod loss;
pub mod materials;
mod native;
pub mod nurbs;
mod protein;
pub mod records;
pub mod sab;
mod tsm;
pub mod validate;
mod writer;
pub mod xref;

use cadmpeg_ir::codec::{Codec, CodecError, Confidence, ContainerSummary, DecodeResult, Encoder};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::source_fidelity::write_plan::{plan_write, WritePlan};
use cadmpeg_ir::Finding;
use std::io::Write;

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

impl Codec for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(ZIP_MAGIC) {
            return Confidence::No;
        }
        // A ZIP alone is a weak signal (many formats are ZIPs). An f3d or f3z
        // marker string in the prefix — entry names are stored in cleartext in
        // ZIP local headers — makes it conclusive.
        if container::DETECT_MARKERS
            .iter()
            .chain(container::F3Z_DETECT_MARKERS)
            .any(|m| memchr::memmem::find(prefix, m).is_some())
        {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root)?;
        Ok(container::summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }

    fn validate_native(&self, ir: &CadIr) -> Vec<Finding> {
        crate::validate::validate_native(ir)
    }
}

impl Encoder for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        writer::generate::write_new(ir, writer)?;
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
        source_fidelity: Option<&cadmpeg_ir::source_fidelity::SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let baseline = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"))
            .map(String::as_str);
        let current = decode::semantic_hash(ir);
        let mut notes = match plan_write(
            source_fidelity,
            ids::FILE_SOURCE_IMAGE_ID,
            baseline,
            &current,
        ) {
            WritePlan::Replay(bytes) => {
                writer.write_all(bytes)?;
                vec!["preserved source container replayed verbatim".to_string()]
            }
            WritePlan::Patch(bytes) => {
                writer::patch::write_semantic(ir, bytes, writer)?;
                vec!["preserved source container patched with supported edits".to_string()]
            }
            WritePlan::Generate => {
                writer::generate::write_new(ir, writer)?;
                let mut notes = vec!["source container regenerated from IR".to_string()];
                // A retained source image was offered but the replay/patch gate
                // could not use it (missing bytes, failed integrity, or absent
                // semantic baseline), so the container was regenerated instead.
                if source_fidelity
                    .and_then(|sidecar| sidecar.retained_record(ids::FILE_SOURCE_IMAGE_ID))
                    .is_some()
                {
                    notes.push(
                        "retained F3D source image was unusable for replay or patch; \
                         regenerated the container from the IR"
                            .to_string(),
                    );
                }
                notes
            }
        };
        notes.push("entity counts are derived from the IR".to_string());

        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "f3d".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes,
        })
    }
}

#[cfg(test)]
mod tests;
