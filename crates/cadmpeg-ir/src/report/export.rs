// SPDX-License-Identifier: Apache-2.0
//! Export reports, write-path fidelity, and entity census.

use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_core::dialect::DialectId;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{LossNote, Severity};

/// Entity census and fidelity details from a successful export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ExportReport {
    /// Target format id.
    pub format: String,
    /// Entity counts and the semantic basis on which they were measured.
    pub census: EntityCensus,
    /// How decode-time source fidelity was handled.
    pub fidelity: FidelityResolution,
    /// Which write path produced the exported bytes.
    pub write_path: WritePath,
    /// Omitted, normalized, or reduced content.
    pub losses: Vec<LossNote>,
    /// Informational details about the export path.
    pub notes: Vec<String>,
    /// The concrete dialect written, including on replay and patch paths, where
    /// the encoder states what the preserved dialect was.
    ///
    /// `None` on exactly one write path, and it stays `Option` for that one:
    /// [`crate::codec::CadirEncoder`] writes the neutral document itself, whose
    /// version is data about cadmpeg and never a dialect, so there is no id to
    /// name. Every native encoder names one on every path, replay and patch
    /// included.
    ///
    /// Always serialized, as `null` when absent. Reports written before the
    /// field existed omit the key and read back `None`.
    #[serde(default)]
    target: Option<DialectId>,
}

/// Which of an encoder's write paths produced the exported bytes.
///
/// An encoder that retains its source bytes has two ways to answer "write this
/// document": copy the retained bytes out, or run the writer. The two are
/// indistinguishable from the output alone whenever the writer happens to
/// reproduce the input, so a round-trip test that only compares bytes cannot say
/// which one it exercised — and a test over an unedited document takes the copy
/// path, proving nothing about the writer. This value is set at the branch the
/// encoder actually took, never derived from the output afterwards, so the
/// distinction is a fact the caller can assert on.
///
/// The variants are ordered by how much of the output the encoder authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WritePath {
    /// Retained source bytes were copied to the output unchanged. No writer code
    /// ran, so the output says nothing about the writer.
    VerbatimReplay,
    /// The writer ran and consumed retained source content, rewriting part of a
    /// container it did not author in full.
    Patched,
    /// The writer ran over neutral IR content alone, authoring every output byte.
    Synthesized,
}

impl fmt::Display for WritePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VerbatimReplay => "verbatim_replay",
            Self::Patched => "patched",
            Self::Synthesized => "synthesized",
        })
    }
}

/// How an encoder resolved optional source fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum FidelityResolution {
    /// The input had no decode-time fidelity state.
    NotProvided,
    /// Preserved source content was consumed successfully.
    Replayed,
    /// The encoder does not consume source fidelity.
    NotConsumed,
    /// Fidelity was available but could not be consumed.
    Degraded {
        /// Explanation of the degradation.
        reason: String,
    },
}

/// The model against which export entity counts were measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CensusBasis {
    /// Counts describe records emitted in the target format.
    TargetRecords,
    /// Counts describe input IR arenas.
    IrArenas,
}

/// Explicitly based entity counts for one export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EntityCensus {
    /// Semantic basis of `counts`.
    pub basis: CensusBasis,
    /// Counts keyed by arena or target-record kind.
    pub counts: BTreeMap<String, usize>,
}

impl EntityCensus {
    /// Total count across every census row.
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

impl ExportReport {
    /// The concrete native dialect written, or `None` for neutral CADIR.
    #[must_use]
    pub fn target(&self) -> Option<&DialectId> {
        self.target.as_ref()
    }

    /// Constructs a report for the neutral CADIR document, which has no native
    /// dialect target.
    #[must_use]
    pub fn cadir(
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            format: "cadir".into(),
            census,
            fidelity,
            write_path,
            losses,
            notes,
            target: None,
        }
    }

    /// Constructs a native-format report with its required dialect target.
    #[must_use]
    pub fn native(
        target: DialectId,
        format: String,
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            format,
            census,
            fidelity,
            write_path,
            losses,
            notes,
            target: Some(target),
        }
    }

    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|loss| loss.severity >= Severity::Error)
            .count()
    }
}
