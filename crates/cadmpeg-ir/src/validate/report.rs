// SPDX-License-Identifier: Apache-2.0
//! Validation vocabulary: the findings a structural and numeric check emits.
//!
//! [`Check`] names the invariant class each validator inspects, a [`Finding`]
//! records one violation with its severity and optional entity id, and a
//! [`ValidationReport`] gathers the findings, per-arena entity counts, and the
//! decode losses propagated into validation for one document. These types
//! describe what validation observed about an already-decoded model and evolve
//! with the checks, independently of the decode and export loss vocabulary.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::report::{LossNote, Severity};

/// Which invariant a validation finding concerns.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// The document schema version is not the version accepted by this build.
    Version,
    /// Entity identifiers are empty, duplicated, or not globally unique.
    Identity,
    /// Product occurrence ownership, references, or acyclicity.
    ProductStructure,
    /// PMI targets and annotation-to-annotation references.
    Pmi,
    /// Presentation-layer membership and references.
    Presentation,
    /// An arena is not sorted lexicographically by entity id.
    ArenaOrder,
    /// A referenced id does not resolve in its arena.
    ReferentialIntegrity,
    /// A face loop's coedge ring does not close.
    LoopClosure,
    /// An edge's two coedges do not pair consistently.
    CoedgePairing,
    /// Wire edges, free vertices, or wire bodies violate topology ownership rules.
    WireTopology,
    /// A face-bearing shell is disconnected through physical-edge incidence.
    ShellTopology,
    /// A geometry carrier cannot be reached from topology or retained construction data.
    CarrierReachability,
    /// An annotation key, stream index, or field path is invalid.
    Annotations,
    /// A source-native namespace record has an unresolved link.
    NativeLinks,
    /// An edge parameter range violates the carrier's canonical domain.
    ParameterDomain,
    /// A document-wide or per-entity tolerance is not sane.
    Tolerances,
    /// A preserved byte payload does not match its declared digest or length.
    PayloadIntegrity,
    /// A tessellation payload is malformed.
    Tessellation,
    /// The document's units are missing or non-canonical, or a tolerance is
    /// invalid.
    Units,
    /// A geometric quantity is out of sane range (e.g. negative radius).
    Bounds,
    /// Evaluated carrier geometry disagrees with the topology it supports:
    /// an edge's curve endpoints or a pcurve's surface image miss the edge's
    /// vertex positions.
    GeometricConsistency,
    /// Arena counts / cross-references are internally inconsistent.
    Counts,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Version => "version",
            Self::Identity => "identity",
            Self::ProductStructure => "product_structure",
            Self::Pmi => "pmi",
            Self::Presentation => "presentation",
            Self::ArenaOrder => "arena_order",
            Self::ReferentialIntegrity => "referential_integrity",
            Self::LoopClosure => "loop_closure",
            Self::CoedgePairing => "coedge_pairing",
            Self::WireTopology => "wire_topology",
            Self::ShellTopology => "shell_topology",
            Self::CarrierReachability => "carrier_reachability",
            Self::Annotations => "annotations",
            Self::NativeLinks => "native_links",
            Self::ParameterDomain => "parameter_domain",
            Self::Tolerances => "tolerances",
            Self::PayloadIntegrity => "payload_integrity",
            Self::Tessellation => "tessellation",
            Self::Units => "units",
            Self::Bounds => "bounds",
            Self::GeometricConsistency => "geometric_consistency",
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

/// Entity counts, findings, and propagated decode losses for one document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    /// Count of entities per arena, keyed by entity kind (sorted).
    pub entity_counts: BTreeMap<String, usize>,
    /// Findings, in discovery order.
    pub findings: Vec<Finding>,
    /// Loss notes supplied to validation.
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
