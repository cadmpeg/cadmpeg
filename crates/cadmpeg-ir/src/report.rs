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

/// Versioned calibration identifiers recorded on every decode (§5.2).
///
/// Profiles and the acceptance envelope carry frozen version tags so an input
/// that begins failing after a constants update keeps a durable explanation
/// instead of a mystery. The decode session sets these authoritatively at
/// [`DecodeContext::finish`](crate::decode::DecodeContext::finish); the
/// [`Default`] value stamps the self-identifying
/// [`UNSTAMPED`](ProfileVersions::UNSTAMPED) sentinel so a report that never
/// reached `finish` is distinguishable from a real calibration rather than
/// masquerading as an all-empty-string match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileVersions {
    /// The active limits profile: `desktop-v1`, `service-v1`, `custom` when the
    /// caller's ceilings match no named profile, or
    /// [`UNSTAMPED`](ProfileVersions::UNSTAMPED) before `finish` stamps it.
    pub profile: String,
    /// The active acceptance-envelope version, e.g. `envelope-v2`, or
    /// [`UNSTAMPED`](ProfileVersions::UNSTAMPED) before `finish` stamps it.
    pub envelope: String,
    /// Caller ceilings that differ from the default desktop profile, each as
    /// `dimension=value`, sorted. Empty when the limits match a named profile;
    /// present only when the profile is `custom`, naming what the caller
    /// changed.
    #[serde(default)]
    pub overrides: Vec<String>,
}

impl ProfileVersions {
    /// The version-slot value stamped into a report that never reached
    /// [`DecodeContext::finish`](crate::decode::DecodeContext::finish). It is a
    /// self-identifying sentinel: a reader diffing two reports can tell an
    /// un-stamped placeholder from a genuine `desktop-v1`/`envelope-v2`
    /// calibration, where an empty string would read as a real match.
    pub const UNSTAMPED: &'static str = "unstamped";
}

impl Default for ProfileVersions {
    fn default() -> Self {
        ProfileVersions {
            profile: Self::UNSTAMPED.to_owned(),
            envelope: Self::UNSTAMPED.to_owned(),
            overrides: Vec::new(),
        }
    }
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
    /// Whether opaque retention degraded from recoverable to accounted because
    /// the retained-byte budget was exhausted in salvage mode (§11.10). Set by
    /// the decode session at `finish`; a paired loss note carries the detail.
    #[serde(default)]
    pub retention_degraded: bool,
    /// Versioned calibration identifiers in force for this decode (§5.2), set
    /// by the decode session at `finish`.
    #[serde(default)]
    pub profile_versions: ProfileVersions,
    /// Validated source-fidelity ledger proving byte conservation over the
    /// source container's physical spaces (§10 Phase 3C), when the codec has
    /// adopted container accounting. `None` for codecs that have not, keeping
    /// the field absent from serialized reports predating the accounting work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fidelity: Option<crate::source_fidelity::SourceFidelity>,
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn report_with(profile_versions: ProfileVersions) -> DecodeReport {
        DecodeReport {
            format: "test".to_string(),
            container_only: false,
            geometry_transferred: false,
            losses: Vec::new(),
            notes: Vec::new(),
            retention_degraded: false,
            profile_versions,
            source_fidelity: None,
        }
    }

    #[test]
    fn profile_versions_serialize_under_stable_keys() {
        // The serialized shape is the durable explanation the feature exists to
        // provide (§5.2): a report keeps a machine-readable record of the
        // constants in force. Lock the wire keys and values so a downstream
        // reader can diff two reports across a library update.
        let report = report_with(ProfileVersions {
            profile: "custom".to_string(),
            envelope: "envelope-v2".to_string(),
            overrides: vec!["max_work=10".to_string()],
        });
        let value: serde_json::Value = serde_json::to_value(&report).unwrap();
        let versions = &value["profile_versions"];
        assert_eq!(versions["profile"], "custom");
        assert_eq!(versions["envelope"], "envelope-v2");
        assert_eq!(versions["overrides"], serde_json::json!(["max_work=10"]));
    }

    #[test]
    fn report_without_profile_versions_deserializes_to_unstamped() {
        // A serialized report predating the field parses via the
        // `#[serde(default)]` back-compat path rather than failing, and the
        // default self-identifies as unstamped — so an old report that carries
        // no calibration is distinguishable from a genuine one, not mistaken
        // for an all-empty-string match.
        let json = r#"{
            "format": "test",
            "container_only": false,
            "geometry_transferred": false,
            "losses": [],
            "notes": []
        }"#;
        let report: DecodeReport = serde_json::from_str(json).unwrap();
        assert_eq!(report.profile_versions, ProfileVersions::default());
        assert_eq!(report.profile_versions.profile, ProfileVersions::UNSTAMPED);
        assert_eq!(report.profile_versions.envelope, ProfileVersions::UNSTAMPED);
        assert!(report.profile_versions.overrides.is_empty());
    }
}
