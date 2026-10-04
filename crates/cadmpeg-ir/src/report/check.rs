// SPDX-License-Identifier: Apache-2.0
//! Validation checks, findings, and reports.

use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::document::CensusKey;
use crate::report::{loss::LossNote, Severity};

/// Which invariant a validation finding concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Check {
    /// An entity id does not match `<format>:<scope>:<kind>#<key>`, or is not
    /// globally unique. A native validator reports a duplicated, unowned, or
    /// uncovered source-native identity here as well.
    Identity,
    /// A PMI target, datum reference, datum system, or semantic-annotation
    /// reference does not resolve.
    Pmi,
    /// A presentation-layer item does not resolve, or an appearance texture
    /// mapping or bump map carries a non-finite value.
    Presentation,
    /// An arena is not strictly sorted by entity id.
    ArenaOrder,
    /// A reference between entities is defective: a typed reference or id does
    /// not resolve; an entity belongs to another owner or sketch; a dependency
    /// or referenced feature does not precede its consumer, or the consumer
    /// omits it from its dependencies; references form a cycle; an identity,
    /// ordinal, address, or range is repeated or invalid; a constraint's
    /// operands are not the kinds the constraint states; a configuration state
    /// disagrees with the feature states it closes over; or an entity schema
    /// cannot state its typed references.
    ReferentialIntegrity,
    /// A coedge radial ring does not close, crosses edges, or, with two
    /// members, states equal coedge senses.
    CoedgePairing,
    /// Wire topology ownership is violated: a coedge also references a wire
    /// edge, an edge also references a free vertex, a wire edge or free vertex
    /// does not belong to exactly one shell, or a wire body contains faces.
    WireTopology,
    /// A face-bearing shell is disconnected through shared edges or vertices.
    ShellTopology,
    /// A surface, curve, pcurve, or point carrier is orphan: no topology or
    /// retained construction data reaches it.
    CarrierReachability,
    /// An annotation provenance or exactness key does not resolve to an entity,
    /// an exactness field path does not resolve, or an entity cannot be
    /// serialized to check its field paths.
    Annotations,
    /// A source-native link is defective: a `native_ref` or native-record link
    /// does not resolve, or a link list is not an array of identity strings. A
    /// codec's native validator also reports here a native record whose frame,
    /// arena census, ordinal run, ownership, or identity disagrees with the
    /// source container.
    NativeLinks,
    /// A parameter range is outside its carrier's canonical domain: an edge
    /// range, a coedge use-curve range, or a coedge pcurve range; or a spatial
    /// sketch NURBS surface states a zero degree.
    ParameterDomain,
    /// A document-wide or per-entity tolerance is outside a sane canonical
    /// range.
    Tolerances,
    /// Preserved bytes do not account for their source: a ledger entry, owner,
    /// or boundary is invalid, a nonempty entry is missing, ranges overlap or
    /// leave a gap, a coverage report is stale or does not prove exact closure,
    /// a preservation record disagrees with the authoritative bytes, or a
    /// carrier census disagrees with the parsed payloads.
    PayloadIntegrity,
    /// A tessellation references a missing body, face, or texture asset.
    Tessellation,
    /// Geometry disagrees with what it supports, or a feature's geometric
    /// inputs are inconsistent: an edge curve, a coedge use-curve, or a pcurve
    /// mapped through its face surface misses the vertex positions it must
    /// meet; a procedural curve does not evaluate at its endpoints or misses
    /// its endpoint or offset-distance contract; a sketch profile is
    /// disconnected, or a sketch constraint, offset pair, locus, or projected
    /// copy has no consistent solution; a feature reference does not name the
    /// kind of geometry it must name, or does not precede its consumer; a
    /// generated vertex, edge, face, body, profile, or region selection is
    /// empty, repeated, out of range, or owned by another sketch; or a
    /// configuration parameter, sheet-metal height, or scale transform is not a
    /// usable value.
    GeometricConsistency,
    /// A count or multiplicity is internally inconsistent: a parameter name or
    /// ordinal repeats within its scope, a design repeats a configuration
    /// ordinal, configuration source index, or feature ordinal, a design states
    /// more than one active configuration, a feature states more than one input
    /// or result topology state, a sketch constraint states the wrong number of
    /// members, or a native record census disagrees with the source container.
    Counts,
}

impl DecodeCost for Check {
    const FIXED_BYTES: Option<u64> = Some(u64_from_index(std::mem::size_of::<Self>()));

    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(u64_from_index(std::mem::size_of::<Self>()))
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Identity => "identity",
            Self::Pmi => "pmi",
            Self::Presentation => "presentation",
            Self::ArenaOrder => "arena_order",
            Self::ReferentialIntegrity => "referential_integrity",
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
            Self::GeometricConsistency => "geometric_consistency",
            Self::Counts => "counts",
        })
    }
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// Which check produced this finding.
    pub check: Check,
    /// Severity.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// The entity id the finding is about, when applicable.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity"
    )]
    pub entity: Option<String>,
}

impl DecodeCost for Finding {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (
            &self.check,
            &self.severity,
            self.message.as_str(),
            &self.entity,
        )
            .decode_cost(ctx, operation)
    }
}

/// Entity counts, findings, and propagated decode losses for one document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ValidationReport {
    /// Count of entities per arena, keyed by entity kind (sorted).
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub entity_counts: BTreeMap<CensusKey, usize>,
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

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_entity, String, "entity");

#[cfg(test)]
mod tests {
    use super::{Check, Finding};
    use crate::report::Severity;
    use cadmpeg_core::decode::cost::DecodeCost;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn finding_cost_includes_fixed_tags_and_owned_text() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let finding = Finding {
            check: Check::Identity,
            severity: Severity::Error,
            message: "bad identity".to_owned(),
            entity: Some("edge#7".to_owned()),
        };
        let expected =
            u64::try_from(std::mem::size_of::<Check>() + std::mem::size_of::<Severity>())
                .expect("test tag sizes fit")
                + u64::try_from(finding.message.len()).expect("test message length fits")
                + 1
                + u64::try_from(finding.entity.as_ref().expect("entity").len())
                    .expect("test entity length fits");
        assert_eq!(
            finding.decode_cost(&ctx, "compare finding").expect("cost"),
            expected
        );
        assert_eq!(
            <Check as DecodeCost>::FIXED_BYTES,
            Some(u64::try_from(std::mem::size_of::<Check>()).expect("test size fits"))
        );
        let without_entity = Finding {
            entity: None,
            message: String::new(),
            ..finding
        };
        assert_eq!(
            without_entity
                .decode_cost(&ctx, "compare finding")
                .expect("cost"),
            u64::try_from(std::mem::size_of::<Check>() + std::mem::size_of::<Severity>())
                .expect("test tag sizes fit")
                + 1
        );
    }

    #[test]
    fn finding_equality_refusal_is_sticky_before_comparison() {
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let finding = Finding {
            check: Check::Counts,
            severity: Severity::Warning,
            message: "count mismatch".to_owned(),
            entity: None,
        };
        let CodecError::ResourceLimit(first) = ctx
            .equal(&finding, &finding, "compare finding")
            .expect_err("finding key work refuses")
        else {
            panic!("resource refusal")
        };
        let CodecError::ResourceLimit(repeated) = ctx
            .equal(&finding, &finding, "repeat finding comparison")
            .expect_err("sticky refusal precedes comparison")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        assert_eq!(ctx.resource_refusal(), Some(first));
    }
}
