// SPDX-License-Identifier: Apache-2.0
//! Loss notes and report types shared by decoding and validation.

use std::collections::BTreeMap;
use std::fmt;

use crate::provenance::Provenance;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity of a loss note or validation finding.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational; no action needed.
    Info,
    /// Something imperfect but non-fatal (e.g. a normalization).
    Warning,
    /// A correctness problem in the produced IR or export.
    Error,
    /// A hard stop: the requested operation cannot be completed faithfully.
    Blocking,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Blocking => "blocking",
        })
    }
}

/// What subsystem a loss pertains to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LossCategory {
    /// Geometry (surfaces/curves/points) not transferred or approximated.
    Geometry,
    /// Topology (graph structure) not transferred.
    Topology,
    /// Materials/appearances not transferred.
    Material,
    /// Document metadata not transferred.
    Metadata,
    /// Units/tolerances issues.
    Units,
    /// Attributes (names, colors, custom attribs) not transferred.
    Attribute,
    /// Anything else.
    Other,
}

impl fmt::Display for LossCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Geometry => "geometry",
            Self::Topology => "topology",
            Self::Material => "material",
            Self::Metadata => "metadata",
            Self::Units => "units",
            Self::Attribute => "attribute",
            Self::Other => "other",
        })
    }
}

/// A single, attributable statement that some information was not carried
/// through faithfully. This is how the transcoder reports loss explicitly
/// rather than silently normalizing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LossNote {
    /// Affected subsystem.
    pub category: LossCategory,
    /// How serious the loss is.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// Where in the source the loss occurred, when attributable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Report produced by a decode: what was transferred and what was lost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeReport {
    /// Source format id.
    pub format: String,
    /// Whether the decode stopped at the container layer (no entity decode).
    pub container_only: bool,
    /// Whether B-rep geometry was actually transferred into the IR. `false`
    /// means the IR has no geometry even if the container was inspected.
    pub geometry_transferred: bool,
    /// Explicit loss notes.
    pub losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    pub notes: Vec<String>,
}

impl DecodeReport {
    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|l| l.severity >= Severity::Error)
            .count()
    }
}

/// Which invariant a validation finding concerns.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// A referenced id does not resolve in its arena.
    ReferentialIntegrity,
    /// A face loop's coedge ring does not close.
    LoopClosure,
    /// An edge's two coedges do not pair consistently.
    CoedgePairing,
    /// The document's units are missing or non-canonical, or a tolerance is
    /// invalid.
    Units,
    /// A geometric quantity is out of sane range (e.g. negative radius).
    Bounds,
    /// Arena counts / cross-references are internally inconsistent.
    Counts,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ReferentialIntegrity => "referential_integrity",
            Self::LoopClosure => "loop_closure",
            Self::CoedgePairing => "coedge_pairing",
            Self::Units => "units",
            Self::Bounds => "bounds",
            Self::Counts => "counts",
        })
    }
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Finding {
    /// Which check produced this finding.
    pub check: Check,
    /// Severity.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// The entity id the finding is about, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
}

/// Report produced by validating an IR document. Carries per-category entity
/// counts, the findings list, and any propagated loss notes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    /// Count of entities per arena, keyed by entity kind (sorted).
    pub entity_counts: BTreeMap<String, usize>,
    /// Findings, in discovery order.
    pub findings: Vec<Finding>,
    /// Loss notes carried alongside validation (e.g. from a prior decode).
    #[serde(default)]
    pub losses: Vec<LossNote>,
}

impl ValidationReport {
    /// Number of findings at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity >= Severity::Error)
            .count()
    }

    /// Number of findings at exactly [`Severity::Warning`].
    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    /// True when there are no [`Severity::Error`]/[`Severity::Blocking`] findings.
    pub fn is_ok(&self) -> bool {
        self.error_count() == 0
    }
}
