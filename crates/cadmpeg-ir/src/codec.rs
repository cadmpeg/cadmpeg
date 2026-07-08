// SPDX-License-Identifier: Apache-2.0
//! The codec contract: how a format plugin detects, inspects, and decodes a
//! source file into the IR.
//!
//! The trait is object-safe so a CLI can hold a registry of `Box<dyn Codec>`
//! and dispatch by detection confidence. I/O is expressed as `&mut dyn
//! ReadSeek` because container formats (ZIP, OLE2) need seeking.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Seek, Write};

use crate::document::CadIr;
use crate::report::DecodeReport;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Blanket read+seek trait so codecs can take a `&mut dyn ReadSeek`.
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
    /// Weak signal (e.g. a generic container magic that many formats share).
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

/// One entry (stream/segment) in a container summary.
///
/// `role` is a codec-defined classification label (e.g. `"brep-smbh"`,
/// `"protein-assets"`); `attributes` carries any extra facts the codec
/// extracted (e.g. an ASM header's magic and version), kept as sorted strings
/// so the summary is generic and canonically serializable.
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
    /// Free-form notes (e.g. selected active BREP entry, caveats).
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
    /// A required capability is not implemented yet; carries a plain-English
    /// explanation of *what* and *why*, surfaced verbatim to the user.
    #[error("not implemented yet: {0}")]
    NotImplemented(String),
    /// Underlying I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A format plugin.
pub trait Codec {
    /// Stable short id for this codec, e.g. `"f3d"`.
    fn id(&self) -> &'static str;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect(&self, prefix: &[u8]) -> Confidence;

    /// Enumerate the container's streams/segments without decoding geometry.
    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError>;

    /// Decode into the IR, reporting loss. Implementations that cannot transfer
    /// geometry must still return a `DecodeResult` whose report makes that
    /// explicit.
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
    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError>;
}
