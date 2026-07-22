// SPDX-License-Identifier: Apache-2.0
//! Decode loss and validation findings.

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
    /// Non-fatal approximation or normalization.
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
    /// Features, sketches, parameters, configurations, or design history not transferred.
    DesignIntent,
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
            Self::DesignIntent => "design_intent",
            Self::Other => "other",
        })
    }
}

/// One attributable instance of incomplete or approximate transfer.
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

/// Transfer status and loss details from a successful decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeReport {
    /// Source format id.
    pub format: String,
    /// Whether the decode stopped at the container layer (no entity decode).
    pub container_only: bool,
    /// Whether the decoder transferred B-rep geometry into the IR.
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

/// Entity census and fidelity details from a successful export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExportReport {
    /// Target format id.
    pub format: String,
    /// Exported entity counts keyed by target entity kind.
    pub entity_counts: BTreeMap<String, usize>,
    /// Total exported entities.
    pub total_entities: usize,
    /// Omitted, normalized, or reduced content.
    pub losses: Vec<LossNote>,
    /// Informational details about the export path.
    pub notes: Vec<String>,
}

impl ExportReport {
    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|loss| loss.severity >= Severity::Error)
            .count()
    }
}

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
    /// Source-byte ownership has a gap, overlap, invalid span, or invalid opaque owner.
    ByteAccounting,
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
            Self::CarrierReachability => "carrier_reachability",
            Self::Annotations => "annotations",
            Self::NativeLinks => "native_links",
            Self::ParameterDomain => "parameter_domain",
            Self::Tolerances => "tolerances",
            Self::PayloadIntegrity => "payload_integrity",
            Self::ByteAccounting => "byte_accounting",
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
