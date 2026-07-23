// SPDX-License-Identifier: Apache-2.0
//! Reads and writes [`cadmpeg_ir::CadIr`] documents as ISO 10303-21 STEP Part
//! 21 exchange structures for AP203, AP214, and AP242.
//!
//! [`write_step`] emits the application protocol selected by
//! [`StepWriteOptions::schema`]. It writes product and representation context,
//! connected exact shape, product occurrences, tessellation, presentation,
//! and PMI when the target schema carries those domains.
//!
//! # Export workflow
//!
//! Construct or decode a [`cadmpeg_ir::CadIr`], choose the header metadata in
//! [`StepWriteOptions`], then write to any [`std::io::Write`] sink:
//!
//! ```
//! use cadmpeg_ir::examples::unit_cube;
//! use cadmpeg_step::{write_step, StepWriteOptions};
//!
//! let ir = unit_cube();
//! let mut bytes = Vec::new();
//! let report = write_step(&ir, &mut bytes, &StepWriteOptions::default())?;
//!
//! assert!(bytes.starts_with(b"ISO-10303-21;"));
//! assert!(report.total_entities > 0);
//! # Ok::<(), cadmpeg_step::StepError>(())
//! ```
//!
//! Review [`cadmpeg_ir::ExportReport::losses`] before retaining report-mode
//! output. [`StepUnsupportedPolicy::Reject`] rejects all such losses before any
//! output byte is written. Opaque records, source attributes, unsupported
//! procedural definitions, and target-schema incompatibilities are reported or
//! rejected rather than silently discarded. Body and face colors become
//! per-face `STYLED_ITEM` presentation; direct geometry and tessellation
//! bindings retain their native presentation targets.
//!
//! Coordinates are emitted unchanged under a millimetre length-unit context.
//! Callers must convert non-millimetre geometry before export. Analytic curves
//! and surfaces map to their corresponding STEP carriers. Rational and
//! non-rational NURBS use the `*_WITH_KNOTS` entities.
//!
//! [`StepError`] represents output-sink failures. Since the writer streams the
//! header and DATA section, such a failure can leave partial output.

mod build;
mod geometry;
pub mod lex;
pub mod parse;
mod reader;
pub mod strings;
mod vocab;
mod writer;

use std::collections::BTreeMap;
use std::io::Write;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeOptions, DecodeResult,
    Encoder,
};
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::CadIr;

use writer::string;

use crate::vocab::FILE_SCHEMA;
use build::Builder;

/// Metadata written to the STEP `FILE_NAME` header record.
///
/// Default values produce deterministic output. They identify the file as
/// `cadmpeg_model`, leave the author and organization empty, use `cadmpeg` as
/// the originating system, and substitute `1970-01-01T00:00:00` for the empty
/// timestamp.
#[derive(Debug, Clone)]
pub struct StepWriteOptions {
    /// Application protocol and edition declared by `FILE_SCHEMA`.
    pub schema: StepSchema,
    /// Handling of IR content the selected writer cannot represent exactly.
    pub unsupported: StepUnsupportedPolicy,
    /// The `FILE_NAME` name field.
    ///
    /// The STEP `PRODUCT` id and name come from the first IR body name, or
    /// `cadmpeg_model` when that body has no name.
    pub product_name: String,
    /// The sole entry in the `FILE_NAME` author list.
    pub author: String,
    /// The sole entry in the `FILE_NAME` organization list.
    pub organization: String,
    /// The `FILE_NAME` timestamp.
    ///
    /// Supply an ISO 8601 value. An empty string is written as
    /// `1970-01-01T00:00:00`.
    pub timestamp: String,
    /// The `FILE_NAME` originating-system field.
    pub originating_system: String,
}

impl Default for StepWriteOptions {
    fn default() -> Self {
        StepWriteOptions {
            schema: StepSchema::Ap214,
            unsupported: StepUnsupportedPolicy::Report,
            product_name: "cadmpeg_model".to_string(),
            author: String::new(),
            organization: String::new(),
            timestamp: String::new(),
            originating_system: "cadmpeg".to_string(),
        }
    }
}

/// Policy for semantic content not representable by the selected STEP target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepUnsupportedPolicy {
    /// Emit the representable subset and return machine-readable loss notes.
    #[default]
    Report,
    /// Reject the document before writing any output byte.
    Reject,
}

/// STEP application-protocol targets supported by the Part 21 writer.
///
/// The AP242 edition number and the long-form schema revision are distinct:
/// editions 1, 2, and 3 use long-form revisions 1, 3, and 4 respectively.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepSchema {
    /// AP203 edition 1 `CONFIG_CONTROL_DESIGN`.
    Ap203Edition1,
    /// AP203 edition 2 modular long form.
    Ap203Edition2,
    /// AP214 `AUTOMOTIVE_DESIGN`.
    #[default]
    Ap214,
    /// AP242 edition 1 modular long form.
    Ap242Edition1,
    /// AP242 edition 2 modular long form.
    Ap242Edition2,
    /// AP242 edition 3 modular long form.
    Ap242Edition3,
}

impl StepSchema {
    /// Exact schema identifier written in `FILE_SCHEMA`.
    pub const fn file_schema(self) -> &'static str {
        match self {
            Self::Ap203Edition1 => "CONFIG_CONTROL_DESIGN",
            Self::Ap203Edition2 => "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }",
            Self::Ap214 => "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }",
            Self::Ap242Edition1 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }",
            Self::Ap242Edition2 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }",
            Self::Ap242Edition3 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }",
        }
    }

    const fn supports_tessellation(self) -> bool {
        matches!(
            self,
            Self::Ap242Edition1 | Self::Ap242Edition2 | Self::Ap242Edition3
        )
    }

    const fn supports_semantic_pmi(self) -> bool {
        self.supports_tessellation()
    }

    const fn supports_visibility(self) -> bool {
        !matches!(self, Self::Ap203Edition1)
    }

    const fn application_protocol(self) -> (&'static str, &'static str, i32) {
        match self {
            Self::Ap203Edition1 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "config_control_design",
                1994,
            ),
            Self::Ap203Edition2 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "ap203_configuration_controlled_3d_design_of_mechanical_parts_and_assemblies",
                2011,
            ),
            Self::Ap214 => ("automotive design", "automotive_design", 2000),
            Self::Ap242Edition1 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2014,
            ),
            Self::Ap242Edition2 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2020,
            ),
            Self::Ap242Edition3 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2022,
            ),
        }
    }
}

/// Failure returned while streaming STEP output.
///
/// Unsupported or reduced IR content appears in [`ExportReport::losses`] after a
/// successful write.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// Strict writing found semantics that would be reduced or omitted.
    #[error("STEP target cannot represent the document without loss: {0}")]
    Unsupported(String),
    /// The output sink rejected a write.
    #[error("failed to write STEP output: {0}")]
    Io(#[from] std::io::Error),
}

/// Serializes an IR document as an ISO 10303-21 STEP AP214 file.
///
/// The output declares the `AUTOMOTIVE_DESIGN` schema and a millimetre length
/// unit. Coordinate values are not rescaled. The IR linear tolerance becomes
/// the representation context's uncertainty value.
///
/// Geometry conversion completes before this function writes the header. It
/// then streams the header, DATA instances, and closing records to `w`. An I/O
/// error can therefore leave a partial file and returns no report.
///
/// On success, the report contains DATA entity counts and loss notes for
/// omitted or reduced content.
pub fn write_step(
    ir: &CadIr,
    w: &mut (impl Write + ?Sized),
    opts: &StepWriteOptions,
) -> Result<ExportReport, StepError> {
    let mut b = Builder::new(ir, opts.schema);
    b.build();
    let report = b.finish_report();
    let lines = b.emitter.into_lines();

    if opts.unsupported == StepUnsupportedPolicy::Reject && !report.losses.is_empty() {
        return Err(StepError::Unsupported(
            report
                .losses
                .iter()
                .map(|loss| loss.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }

    write_header(w, opts)?;
    writeln!(w, "DATA;")?;
    for line in &lines {
        writeln!(w, "{line}")?;
    }
    writeln!(w, "ENDSEC;")?;
    writeln!(w, "END-ISO-10303-21;")?;
    Ok(report)
}

fn write_header(w: &mut (impl Write + ?Sized), opts: &StepWriteOptions) -> std::io::Result<()> {
    let ts = if opts.timestamp.is_empty() {
        "1970-01-01T00:00:00"
    } else {
        &opts.timestamp
    };
    writeln!(w, "ISO-10303-21;")?;
    writeln!(w, "HEADER;")?;
    writeln!(
        w,
        "FILE_DESCRIPTION(({}),'2;1');",
        string("CAD model exported by cadmpeg")
    )?;
    writeln!(
        w,
        "FILE_NAME({},{},({}),({}),{},{},{});",
        string(&opts.product_name),
        string(ts),
        string(&opts.author),
        string(&opts.organization),
        string("cadmpeg-step"),
        string(&opts.originating_system),
        string("")
    )?;
    writeln!(w, "FILE_SCHEMA(({}));", string(opts.schema.file_schema()))?;
    writeln!(w, "ENDSEC;")?;
    Ok(())
}

/// STEP Part 21 writer configured with per-export header options.
#[derive(Debug, Clone, Default)]
pub struct StepEncoder {
    /// Header metadata and deterministic writer options.
    pub options: StepWriteOptions,
}

impl Encoder for StepEncoder {
    fn id(&self) -> &'static str {
        "step"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        write_step(ir, writer, &self.options).map_err(CodecError::from)
    }
}

/// STEP Part 21 reader for ISO 10303-21 exchange structures.
#[derive(Debug, Clone, Copy, Default)]
pub struct StepCodec;

impl Codec for StepCodec {
    fn id(&self) -> &'static str {
        "step"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if prefix.starts_with(b"ISO-10303-21;") {
            Confidence::High
        } else if is_part28_xml(prefix) {
            Confidence::Medium
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        _ctx: &cadmpeg_ir::decode::DecodeContext<'_>,
        root: cadmpeg_ir::decode::View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let bytes = root.window();
        refuse_alternate_encoding(bytes)?;
        if self.detect(bytes) == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        let exchange =
            parse::parse(bytes).map_err(|error| CodecError::Malformed(error.to_string()))?;
        let (decoded, opaque_offsets) = reader::inspect_exchange(bytes, &exchange);
        let mut entries = vec![ContainerEntry {
            name: "HEADER".into(),
            role: "metadata".into(),
            compression: "none".into(),
            compressed_size: 0,
            uncompressed_size: 0,
            attributes: BTreeMap::default(),
        }];
        if !exchange.anchors.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert("anchor_count".into(), exchange.anchors.len().to_string());
            entries.push(ContainerEntry {
                name: "ANCHOR".into(),
                role: "in_file_anchors".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        if !exchange.references.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert(
                "external_count".into(),
                exchange.references.len().to_string(),
            );
            attributes.insert(
                "external_uris".into(),
                exchange
                    .references
                    .iter()
                    .map(|entry| entry.uri.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            entries.push(ContainerEntry {
                name: "REFERENCE".into(),
                role: "external_references".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        for (index, section) in exchange.data.iter().enumerate() {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for id in &section.records {
                if !opaque_offsets.contains(&exchange.records[id].span.start) {
                    continue;
                }
                for partial in &exchange.records[id].partials {
                    *counts.entry(partial.name.clone()).or_default() += 1;
                }
            }
            let unknown = counts
                .iter()
                .map(|(name, count)| format!("{name}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert("entity_count".into(), section.records.len().to_string());
            attributes.insert("unknown_entities".into(), unknown);
            entries.push(ContainerEntry {
                name: format!("DATA[{index}]"),
                role: "entity_records".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        let external_dependencies = decoded
            .report
            .notes
            .iter()
            .filter(|note| {
                note.starts_with("external document ") || note.starts_with("external source ")
            })
            .cloned()
            .collect::<Vec<_>>();
        if !external_dependencies.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert(
                "dependency_count".into(),
                external_dependencies.len().to_string(),
            );
            attributes.insert("dependencies".into(), external_dependencies.join(","));
            entries.push(ContainerEntry {
                name: "EXTERNAL_DEPENDENCIES".into(),
                role: "external_references".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        if exchange.signature.is_some() {
            entries.push(ContainerEntry {
                name: "SIGNATURE".into(),
                role: "signature".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes: BTreeMap::default(),
            });
        }
        let schema = exchange
            .header
            .iter()
            .find(|record| record.name == FILE_SCHEMA)
            .map_or_else(
                || "unspecified".into(),
                |record| {
                    fn strings(value: &parse::Value, out: &mut Vec<String>) {
                        match value {
                            parse::Value::String(bytes) => {
                                if let Ok(value) = strings::decode(bytes) {
                                    out.push(value);
                                }
                            }
                            parse::Value::List(values) => {
                                for value in values {
                                    strings(value, out);
                                }
                            }
                            parse::Value::Typed(_, value) => strings(value, out),
                            _ => {}
                        }
                    }
                    let mut names = Vec::new();
                    record
                        .parameters
                        .iter()
                        .for_each(|value| strings(value, &mut names));
                    names.join(",")
                },
            );
        let edition = if schema.contains("442 4") {
            "edition 3"
        } else if schema.contains("442 3") {
            "edition 2"
        } else if schema.contains("442 1") {
            "edition 1"
        } else {
            "edition unspecified"
        };
        Ok(ContainerSummary {
            format: "step".into(),
            container_kind: "iso-10303-21-clear-text".into(),
            entries,
            notes: vec![format!("schema {schema}; {edition}")],
        })
    }

    fn decode_impl(
        &self,
        ctx: &cadmpeg_ir::decode::DecodeContext<'_>,
        root: cadmpeg_ir::decode::View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let bytes = root.window();
        refuse_alternate_encoding(bytes)?;
        if self.detect(bytes) == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        reader::decode(
            bytes,
            DecodeOptions {
                container_only: ctx.container_only(),
                policy: *ctx.policy(),
            },
        )
    }
}

fn refuse_alternate_encoding(bytes: &[u8]) -> Result<(), CodecError> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Err(CodecError::NotImplemented(
            "STEP Part 21 ZIP container".into(),
        ));
    }
    if bytes.starts_with(b"\x89HDF\r\n\x1a\n") {
        return Err(CodecError::NotImplemented(
            "STEP Part 26 binary/HDF5 encoding".into(),
        ));
    }
    if is_part28_xml(bytes) {
        return Err(CodecError::NotImplemented(
            "STEP Part 28 XML encoding".into(),
        ));
    }
    let lower = bytes
        .iter()
        .take(4096)
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if lower.starts_with(b"<?xml")
        && (lower
            .windows(21)
            .any(|window| window == b"business_object_model")
            || lower.windows(14).any(|window| window == b"ap242_bo_model"))
    {
        return Err(CodecError::NotImplemented(
            "AP242 BO-Model XML sidecar".into(),
        ));
    }
    Ok(())
}

fn is_part28_xml(bytes: &[u8]) -> bool {
    let lower = bytes
        .iter()
        .take(4096)
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    lower.starts_with(b"<?xml")
        && (lower.windows(12).any(|window| window == b"iso_10303_28")
            || lower
                .windows(21)
                .any(|window| window == b"iso:std:iso:10303:-28"))
}

impl From<StepError> for CodecError {
    fn from(error: StepError) -> Self {
        match error {
            StepError::Unsupported(message) => Self::NotImplemented(message),
            StepError::Io(error) => Self::Io(error),
        }
    }
}

#[cfg(test)]
mod tests;
