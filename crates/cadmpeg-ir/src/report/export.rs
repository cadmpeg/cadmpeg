// SPDX-License-Identifier: Apache-2.0
//! Export reports, write-path fidelity, and entity census.

use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_core::dialect::DialectId;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{LossNote, Severity};
use crate::codec::write::WritePath as BackendWritePath;
use crate::document::CensusKey;

/// Entity census and fidelity details from a successful export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExportReport {
    identity: ExportIdentity,
    /// Entity counts and the semantic basis on which they were measured.
    pub census: EntityCensus,
    write_path: WritePath,
    /// Omitted, normalized, or reduced content.
    pub losses: Vec<LossNote>,
    /// Informational details about the export path.
    pub notes: Vec<String>,
}

/// What an export report describes: the canonical CADIR document, or one
/// native dialect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "payload", rename_all = "snake_case", deny_unknown_fields)]
enum ExportIdentity {
    /// The dialect-free canonical CADIR document.
    Cadir {},
    /// A current native export, identified by its resolved target.
    Native {
        /// Resolved native dialect written.
        target: DialectId,
    },
}

#[cfg(all(test, feature = "schema"))]
mod schema_tests {
    #[test]
    fn the_native_export_payload_schema_requires_its_target() {
        let schema = serde_json::to_value(schemars::schema_for!(super::ExportIdentity))
            .expect("export identity schema serializes");
        let native = schema["oneOf"]
            .as_array()
            .expect("export identity schema is a tagged union")
            .iter()
            .find(|arm| arm["properties"]["payload"]["const"] == "native")
            .expect("the native arm");
        let required = native["required"]
            .as_array()
            .expect("the native arm has required fields");
        assert!(required.iter().any(|field| field == "target"), "{schema:#}");
    }
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
/// Each arm carries the fidelity resolutions that path admits, so a path and a
/// resolution it can never produce have no spelling.
///
/// The variants are ordered by how much of the output the encoder authored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "path", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum WritePath {
    /// Retained source bytes were copied to the output unchanged. No writer code
    /// ran, so the output says nothing about the writer.
    VerbatimReplay {
        /// How the replay resolved decode-time source fidelity.
        fidelity: ReplayFidelity,
    },
    /// The writer ran and consumed retained source content, rewriting part of a
    /// container it did not author in full.
    Patched {
        /// How the patch resolved decode-time source fidelity.
        fidelity: FidelityResolution,
    },
    /// The writer ran over neutral IR content alone, authoring every output byte.
    Synthesized {
        /// How the synthesized write resolved decode-time source fidelity.
        fidelity: SynthesisFidelity,
    },
}

impl fmt::Display for WritePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VerbatimReplay { .. } => "verbatim_replay",
            Self::Patched { .. } => "patched",
            Self::Synthesized { .. } => "synthesized",
        })
    }
}

/// The fidelity resolutions a verbatim replay admits.
///
/// A replay copies retained source bytes, so it either consumed the fidelity it
/// was given or was given none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "status")]
#[serde(deny_unknown_fields)]
pub enum ReplayFidelity {
    /// The input had no decode-time fidelity state.
    NotProvided {},
    /// Preserved source content was consumed successfully.
    Replayed {},
}

impl From<ReplayFidelity> for FidelityResolution {
    fn from(fidelity: ReplayFidelity) -> Self {
        match fidelity {
            ReplayFidelity::NotProvided {} => Self::NotProvided {},
            ReplayFidelity::Replayed {} => Self::Replayed {},
        }
    }
}

/// The fidelity resolutions a synthesized write admits.
///
/// Synthesis authors every output byte from neutral IR, so it never replays
/// source content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "status")]
#[serde(deny_unknown_fields)]
pub enum SynthesisFidelity {
    /// The input had no decode-time fidelity state.
    NotProvided {},
    /// The encoder does not consume source fidelity.
    NotConsumed {},
    /// Fidelity was available but could not be consumed.
    Degraded {
        /// Explanation of the degradation.
        reason: String,
    },
}

impl From<SynthesisFidelity> for FidelityResolution {
    fn from(fidelity: SynthesisFidelity) -> Self {
        match fidelity {
            SynthesisFidelity::NotProvided {} => Self::NotProvided {},
            SynthesisFidelity::NotConsumed {} => Self::NotConsumed {},
            SynthesisFidelity::Degraded { reason } => Self::Degraded { reason },
        }
    }
}

/// How an encoder resolved optional source fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "status")]
#[serde(deny_unknown_fields)]
pub enum FidelityResolution {
    /// The input had no decode-time fidelity state.
    NotProvided {},
    /// Preserved source content was consumed successfully.
    Replayed {},
    /// The encoder does not consume source fidelity.
    NotConsumed {},
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
#[serde(deny_unknown_fields)]
pub enum CensusBasis {
    /// Counts describe records emitted in the target format.
    TargetRecords,
    /// Counts describe input IR arenas.
    IrArenas,
}

/// Explicitly based entity counts for one export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EntityCensus {
    /// Semantic basis of `counts`.
    pub basis: CensusBasis,
    /// Counts keyed by arena or target-record kind.
    pub counts: BTreeMap<CensusKey, usize>,
}

impl EntityCensus {
    /// Total count across every census row.
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

impl ExportReport {
    /// How decode-time source fidelity was handled.
    #[must_use]
    pub fn fidelity(&self) -> FidelityResolution {
        match &self.write_path {
            WritePath::VerbatimReplay { fidelity } => fidelity.clone().into(),
            WritePath::Patched { fidelity } => fidelity.clone(),
            WritePath::Synthesized { fidelity } => fidelity.clone().into(),
        }
    }

    /// Which write path produced the exported bytes, and how that path resolved
    /// source fidelity.
    #[must_use]
    pub fn write_path(&self) -> &WritePath {
        &self.write_path
    }

    /// Returns the native format namespace, or `"cadir"` for neutral CADIR.
    #[must_use]
    pub fn format(&self) -> &str {
        match &self.identity {
            ExportIdentity::Cadir {} => "cadir",
            ExportIdentity::Native { target } => target.namespace(),
        }
    }

    /// The concrete native dialect written.
    ///
    /// `None` identifies neutral CADIR. Native reports always name a target.
    #[must_use]
    pub fn target(&self) -> Option<&DialectId> {
        match &self.identity {
            ExportIdentity::Native { target } => Some(target),
            ExportIdentity::Cadir {} => None,
        }
    }

    /// Constructs a report for the neutral CADIR document, which has no native
    /// dialect target.
    #[must_use]
    pub(crate) fn cadir(
        census: EntityCensus,
        write_path: BackendWritePath,
        fidelity_provided: bool,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            identity: ExportIdentity::Cadir {},
            census,
            write_path: write_path.into_report(fidelity_provided),
            losses,
            notes,
        }
    }

    /// Constructs a native-format report with its required dialect target.
    ///
    #[must_use]
    pub(crate) fn native(
        target: DialectId,
        census: EntityCensus,
        write_path: BackendWritePath,
        fidelity_provided: bool,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            identity: ExportIdentity::Native { target },
            census,
            write_path: write_path.into_report(fidelity_provided),
            losses,
            notes,
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
