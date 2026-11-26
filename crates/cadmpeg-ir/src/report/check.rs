// SPDX-License-Identifier: Apache-2.0
//! Validation checks, findings, and reports.

use cadmpeg_core::text::NonBlankString;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::document::CensusKey;
use crate::report::{loss::LossNote, Severity};

/// Which invariant a validation finding concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Check {
    /// A whole-document check was skipped for a fragment.
    FragmentSkipped,
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
    /// disagrees with the feature states it closes over; an entity schema
    /// cannot state its typed references; or an occurrence places the root
    /// definition.
    ReferentialIntegrity,
    /// A coedge radial ring does not close, crosses edges, or, with two
    /// members, states equal coedge senses.
    CoedgePairing,
    /// Wire topology ownership is violated: a coedge also references a wire
    /// edge, an edge also references a free vertex, a wire edge, free vertex,
    /// or subdivision cage does not belong to exactly one shell, a shell
    /// repeats a member, a shell holding a cage holds anything else or is not
    /// its body's only shell, or a wire body contains faces.
    WireTopology,
    /// A face-bearing shell is disconnected through shared edges or vertices.
    ShellTopology,
    /// A face does not state its boundary in full, or a region, shell, or body does
    /// not state every child (§5.6, Unstated children).
    TopologyPartial,
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
    /// members, a native record census disagrees with the source container, or
    /// a document states more than one root definition, or a complete decoded
    /// document none.
    Counts,
    /// A face's edge loop does not list its coedges in traversal order: its
    /// coedges form one closed traversal by vertex identity in another order,
    /// several closed traversals, or none.
    RingOrder,
    /// A coedge does not list its pcurve uses in traversal order: they form
    /// one chain from the coedge's end to its start.
    PcurveOrder,
    /// A face's sense disagrees with the winding of its loops, or the faces
    /// of a body whose shells are closed disagree on its volume: a face's
    /// readings or a shell's volume sign contradict the body's orientation,
    /// or the flux of the unit field through the shells exceeds its
    /// tolerance.
    Orientation,
    /// The measurement findings pass ran out of its allowance before it
    /// checked every face, edge and closed body for [`Self::RingOrder`],
    /// [`Self::PcurveOrder`] and [`Self::Orientation`].
    MeasurementIncomplete,
    /// A quantity that the source declares for its bodies differs from the
    /// quantity measured on the decoded geometry by more than the
    /// measurement's error terms, cannot be measured, or names no bodies.
    DeclaredMeasure,
    /// A record of the design layer breaks a rule of part 10 of the CADIR
    /// v11 specification: a scope invariant, the container tree, a position,
    /// an evaluation key or base, a current state, a resolved state, or a
    /// configuration, arrangement, or override.
    Design,
    /// A record of the process layer breaks a rule of part 10 of the CADIR
    /// v11 specification: an operation's setup membership, a step, a
    /// reference between process records, a toolpath or process state key,
    /// a binding, or a setting stated off its operation's class.
    Process,
}

impl Check {
    /// The check's wire name, the open name a finding's signature states
    /// (§2.7).
    #[must_use]
    pub fn wire_name(self) -> NonBlankString {
        match self {
            Self::FragmentSkipped => cadmpeg_core::nonblank_literal!("fragment_skipped"),
            Self::Identity => cadmpeg_core::nonblank_literal!("identity"),
            Self::Pmi => cadmpeg_core::nonblank_literal!("pmi"),
            Self::Presentation => cadmpeg_core::nonblank_literal!("presentation"),
            Self::ArenaOrder => cadmpeg_core::nonblank_literal!("arena_order"),
            Self::ReferentialIntegrity => cadmpeg_core::nonblank_literal!("referential_integrity"),
            Self::CoedgePairing => cadmpeg_core::nonblank_literal!("coedge_pairing"),
            Self::WireTopology => cadmpeg_core::nonblank_literal!("wire_topology"),
            Self::ShellTopology => cadmpeg_core::nonblank_literal!("shell_topology"),
            Self::TopologyPartial => cadmpeg_core::nonblank_literal!("topology_partial"),
            Self::CarrierReachability => cadmpeg_core::nonblank_literal!("carrier_reachability"),
            Self::Annotations => cadmpeg_core::nonblank_literal!("annotations"),
            Self::NativeLinks => cadmpeg_core::nonblank_literal!("native_links"),
            Self::ParameterDomain => cadmpeg_core::nonblank_literal!("parameter_domain"),
            Self::Tolerances => cadmpeg_core::nonblank_literal!("tolerances"),
            Self::PayloadIntegrity => cadmpeg_core::nonblank_literal!("payload_integrity"),
            Self::Tessellation => cadmpeg_core::nonblank_literal!("tessellation"),
            Self::GeometricConsistency => cadmpeg_core::nonblank_literal!("geometric_consistency"),
            Self::Counts => cadmpeg_core::nonblank_literal!("counts"),
            Self::RingOrder => cadmpeg_core::nonblank_literal!("ring_order"),
            Self::PcurveOrder => cadmpeg_core::nonblank_literal!("pcurve_order"),
            Self::Orientation => cadmpeg_core::nonblank_literal!("orientation"),
            Self::MeasurementIncomplete => {
                cadmpeg_core::nonblank_literal!("measurement_incomplete")
            }
            Self::DeclaredMeasure => cadmpeg_core::nonblank_literal!("declared_measure"),
            Self::Design => cadmpeg_core::nonblank_literal!("design"),
            Self::Process => cadmpeg_core::nonblank_literal!("process"),
        }
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name().as_str())
    }
}

/// The name of one validation rule, `[<codec>/]<area>.<detail>`: `<area>`
/// is the validator module that states the rule, `<detail>` kebab-case words.
/// It is stated once, where the rule is checked, and does not change when
/// the rule's message does.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RuleName(Cow<'static, str>);

impl RuleName {
    /// The name `name`.
    ///
    /// # Panics
    ///
    /// Panics where `name` does not have the grammar of a rule name; in a
    /// constant, as [`rule_name!`](crate::rule_name) builds it, the crate
    /// does not build.
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        assert!(
            is_rule_name(name.as_bytes()),
            "a rule name is [<codec>/]<area>.<detail>"
        );
        Self(Cow::Borrowed(name))
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `[<codec>/]<area>.<detail>`: an optional kebab-case codec and `/`, an
/// area of lower-case letters, digits, and `_` that starts with a letter, a
/// `.`, and kebab-case words of lower-case letters and digits.
const fn is_rule_name(bytes: &[u8]) -> bool {
    const fn word(byte: u8) -> bool {
        byte.is_ascii_lowercase() || byte.is_ascii_digit()
    }
    const fn kebab(bytes: &[u8], start: usize, end: usize) -> bool {
        if start >= end || bytes[start] == b'-' || bytes[end - 1] == b'-' {
            return false;
        }
        let mut index = start;
        while index < end {
            let byte = bytes[index];
            if !(word(byte) || byte == b'-' && bytes[index - 1] != b'-') {
                return false;
            }
            index += 1;
        }
        true
    }
    let mut slash = None;
    let mut dot = None;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if slash.is_none() && dot.is_none() => slash = Some(index),
            b'.' if dot.is_none() => dot = Some(index),
            _ => {}
        }
        index += 1;
    }
    let Some(dot) = dot else {
        return false;
    };
    let area = match slash {
        Some(slash) => {
            if !kebab(bytes, 0, slash) {
                return false;
            }
            slash + 1
        }
        None => 0,
    };
    if area >= dot || !bytes[area].is_ascii_lowercase() {
        return false;
    }
    let mut index = area;
    while index < dot {
        let byte = bytes[index];
        if !(word(byte) || byte == b'_') {
            return false;
        }
        index += 1;
    }
    kebab(bytes, dot + 1, bytes.len())
}

impl fmt::Display for RuleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RuleName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if is_rule_name(name.as_bytes()) {
            Ok(Self(Cow::Owned(name)))
        } else {
            Err(serde::de::Error::custom(format_args!(
                "`{name}` is not a rule name"
            )))
        }
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for RuleName {
    fn schema_name() -> Cow<'static, str> {
        "RuleName".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": r"^([a-z0-9]+(-[a-z0-9]+)*/)?[a-z][a-z0-9_]*\.[a-z0-9]+(-[a-z0-9]+)*$"
        })
    }
}

/// A finding's message and the rule it reports. [`rule!`](crate::rule)
/// builds one.
#[derive(Debug, Clone, Copy)]
pub struct Ruled<'a> {
    /// The rule.
    pub rule: &'a RuleName,
    /// The message.
    pub message: fmt::Arguments<'a>,
}

/// A rule name from a literal, checked when the crate builds.
///
/// ```compile_fail
/// # fn main() {
/// let _ = cadmpeg_ir::rule_name!("Not A Rule");
/// # }
/// ```
#[macro_export]
macro_rules! rule_name {
    ($name:literal) => {
        const { $crate::report::check::RuleName::from_static($name) }
    };
}

/// A finding's message with the rule it reports: `rule!("area.detail",
/// "message {}", value)`.
#[macro_export]
macro_rules! rule {
    ($name:literal, $($message:tt)+) => {
        $crate::report::check::Ruled {
            rule: &$crate::rule_name!($name),
            message: format_args!($($message)+),
        }
    };
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// Which check produced this finding.
    pub check: Check,
    /// The rule the finding reports.
    pub rule: RuleName,
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
    /// Other document entities a finding about `entity` involves, in message
    /// order. Empty when `entity` is absent.
    pub related: Vec<String>,
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

    #[test]
    fn finding_without_related_writes_an_empty_list() -> Result<(), serde_json::Error> {
        let wire = serde_json::json!({
            "check": "referential_integrity",
            "rule": "test.invalid-reference",
            "severity": "error",
            "message": "invalid reference",
            "entity": "test:model:feature#consumer",
            "related": []
        });
        let finding: Finding = cadmpeg_core::serde_erase::json::from_value(wire.clone())?;
        assert!(finding.related.is_empty());
        assert_eq!(cadmpeg_core::serde_erase::json::to_value(finding)?, wire);
        Ok(())
    }

    #[test]
    fn a_finding_names_its_rule_in_the_rule_grammar() {
        let wire = |rule: &str| {
            serde_json::json!({
                "check": "referential_integrity",
                "rule": rule,
                "severity": "error",
                "message": "invalid reference",
                "related": []
            })
        };
        for good in [
            "topology.free-vertex",
            "f3d/links.native-ref",
            "design_x.a1",
        ] {
            let finding: Finding =
                cadmpeg_core::serde_erase::json::from_value(wire(good)).expect(good);
            assert_eq!(finding.rule.as_str(), good);
        }
        for bad in [
            "topology",
            "Topology.x",
            "topology.-x",
            "a/b/c.d",
            "topology.x_y",
            "",
        ] {
            assert!(
                cadmpeg_core::serde_erase::json::from_value::<Finding>(wire(bad)).is_err(),
                "{bad}"
            );
        }
        let mut missing = wire("topology.free-vertex");
        missing
            .as_object_mut()
            .expect("a finding is an object")
            .remove("rule");
        assert!(cadmpeg_core::serde_erase::json::from_value::<Finding>(missing).is_err());
    }

    #[test]
    fn measurement_checks_are_pinned() -> Result<(), serde_json::Error> {
        for (check, name) in [
            (Check::RingOrder, "ring_order"),
            (Check::PcurveOrder, "pcurve_order"),
            (Check::Orientation, "orientation"),
            (Check::MeasurementIncomplete, "measurement_incomplete"),
            (Check::DeclaredMeasure, "declared_measure"),
        ] {
            assert_eq!(cadmpeg_core::serde_erase::json::to_value(check)?, name);
            assert_eq!(check.to_string(), name);
            assert_eq!(
                cadmpeg_core::serde_erase::json::from_value::<Check>(name.into())?,
                check
            );
        }
        Ok(())
    }

    #[test]
    fn finding_round_trips_related_ids_in_order() -> Result<(), serde_json::Error> {
        let finding = Finding {
            rule: crate::rule_name!("test.invalid-dependencies"),
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: "invalid dependencies".into(),
            entity: Some("test:model:feature#consumer".into()),
            related: vec![
                "test:model:feature#second".into(),
                "test:model:feature#first".into(),
            ],
        };
        let wire = cadmpeg_core::serde_erase::json::to_value(&finding)?;
        assert_eq!(wire["related"], serde_json::json!(finding.related));
        assert_eq!(
            cadmpeg_core::serde_erase::json::from_value::<Finding>(wire)?,
            finding
        );
        Ok(())
    }
}
