// SPDX-License-Identifier: Apache-2.0
//! Interfaces for detecting, inspecting, decoding, and encoding CAD formats.
//!
//! [`Codec`] is object-safe for runtime codec registries. Detection consumes a
//! byte prefix, inspection summarizes a seekable container, and decoding
//! produces a finalized [`CadIr`] plus a [`DecodeReport`].

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Seek, Write};

use crate::document::CadIr;
use crate::report::DecodeReport;
use crate::report::ExportReport;
use crate::source_fidelity::SourceFidelity;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Object-safe input bound combining [`Read`] and [`Seek`].
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// How confident a codec is that it can handle a given byte prefix.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Definitely not this format.
    No,
    /// Weak signal, such as a generic container signature.
    Low,
    /// Plausible but not conclusive.
    Medium,
    /// Strong, format-specific signal.
    High,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::No => "no",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
    }
}

/// One stream or segment in a container summary.
///
/// `role` and `attributes` are codec-defined. The ordered attribute map keeps
/// the format-independent summary deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerEntry {
    /// Entry name/path within the container.
    pub name: String,
    /// Codec-defined role classification.
    pub role: String,
    /// Compression method label (e.g. `"stored"`, `"deflate"`, `"zstd"`).
    pub compression: String,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Uncompressed size in bytes.
    pub uncompressed_size: u64,
    /// Extra codec-extracted attributes, sorted by key.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// The result of inspecting a container without decoding its geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerSummary {
    /// Source format id.
    pub format: String,
    /// Container kind, e.g. `"zip"`.
    pub container_kind: String,
    /// Enumerated entries.
    pub entries: Vec<ContainerEntry>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
}

/// Options controlling source decoding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeOptions {
    /// Stop after the container layer; do not attempt entity decode.
    pub container_only: bool,
}

/// A decoded document plus its loss report.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeResult {
    /// The decoded IR.
    pub ir: CadIr,
    /// What was transferred and what was lost.
    pub report: DecodeReport,
    /// Decode-time byte accounting and conversion facts.
    pub source_fidelity: SourceFidelity,
}

impl DecodeResult {
    /// Build a result after canonicalizing the document's arena order.
    pub fn new(mut ir: CadIr, report: DecodeReport) -> Self {
        ir.finalize();
        Self {
            ir,
            report,
            source_fidelity: SourceFidelity::default(),
        }
    }

    /// Build a result with an explicit source-fidelity sidecar.
    pub fn with_source_fidelity(
        mut ir: CadIr,
        report: DecodeReport,
        mut source_fidelity: SourceFidelity,
    ) -> Self {
        ir.finalize();
        source_fidelity.finalize();
        Self {
            ir,
            report,
            source_fidelity,
        }
    }
}

/// Errors a codec can raise.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// The bytes are not this codec's format.
    #[error("not the expected format: {0}")]
    WrongFormat(String),
    /// The container was structurally malformed.
    #[error("malformed container: {0}")]
    Malformed(String),
    /// The codec does not implement a required capability.
    #[error("not implemented yet: {0}")]
    NotImplemented(String),
    /// Underlying I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<crate::native::NativeConvertError> for CodecError {
    fn from(error: crate::native::NativeConvertError) -> Self {
        Self::Malformed(error.to_string())
    }
}

/// Decoder and container inspector for one source format.
pub trait Codec {
    /// Stable short id for this codec, e.g. `"f3d"`.
    fn id(&self) -> &'static str;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect(&self, prefix: &[u8]) -> Confidence;

    /// Enumerate the container's streams/segments without decoding geometry.
    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError>;

    /// Decode the source and report any incomplete or approximate transfer.
    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError>;
}

/// A native-format writer.
pub trait Encoder {
    /// Stable output format id.
    fn id(&self) -> &'static str;

    /// Encode one IR document to the target format.
    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError>;

    /// Encode with decode-time source fidelity when the caller retained it.
    ///
    /// Encoders that do not consume source accounting use the neutral model
    /// through [`Encoder::encode`].
    fn encode_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: Option<&SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let _ = source_fidelity;
        self.encode(ir, writer)
    }
}

/// Encoder for canonical versioned CADIR JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct CadirEncoder;

impl Encoder for CadirEncoder {
    fn id(&self) -> &'static str {
        "cadir"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        let mut json = ir
            .to_canonical_json()
            .map_err(|error| CodecError::Malformed(error.to_string()))?;
        json.push('\n');
        writer.write_all(json.as_bytes())?;
        let validation = crate::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "cadir".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: Vec::new(),
        })
    }
}
