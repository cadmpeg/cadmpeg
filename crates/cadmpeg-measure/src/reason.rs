// SPDX-License-Identifier: Apache-2.0
//! The pinned measurement vocabulary: categories, unavailable reasons,
//! caveats and comparison causes.
//!
//! Every reason and caveat serializes as a `family.detail` string.
//! The strings are the contract: a code is never renamed, removed, or given
//! another meaning or category. A new path that makes a value unavailable
//! adds a variant. Readers dispatch on the closed [`Category`] set and treat
//! an unknown code through its category.

use serde::{Serialize, Serializer};

/// The closed set of reasons a value is unavailable or carries a caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The document's topology or geometry is missing, inconsistent, or
    /// contradicts itself for this quantity.
    Input,
    /// The quantity does not exist for the entity as the document states it,
    /// or the document carries no data for the fact.
    Undefined,
    /// This build has no method for the quantity on this entity, or cannot
    /// confirm the reading it reports.
    Unsupported,
    /// A local work, sampling or quadrature cap, or the pass allowance,
    /// stopped the computation.
    Budget,
    /// The entity's composed placement prevents the quantity.
    Placement,
}

impl Category {
    /// Every category, in declaration order.
    pub const ALL: [Self; 5] = [
        Self::Input,
        Self::Undefined,
        Self::Unsupported,
        Self::Budget,
        Self::Placement,
    ];

    /// The serialized name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Undefined => "undefined",
            Self::Unsupported => "unsupported",
            Self::Budget => "budget",
            Self::Placement => "placement",
        }
    }
}

/// Why a value is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnavailableReason {
    /// The carrier is stored as an unknown shape.
    CarrierUnknown,
    /// The referenced carrier ID does not resolve.
    CarrierUnresolved,
    /// A procedural carrier has no evaluator result and no usable cache.
    CarrierProceduralUnevaluated,
    /// A polygonal carrier; bounded measures are not computed.
    CarrierPolygonal,
    /// A composite curve has no parameterization.
    CarrierCompositeCurve,
    /// Area or flux on a cone whose cross-section is an ellipse.
    CarrierEllipticCone,
    /// The composed placement is not a similarity.
    PlacementNonSimilarity,
    /// Transform composition left the finite range.
    PlacementNonFinite,
    /// The face's region is unbounded.
    BoundaryUnboundedFace,
    /// The ring's coedges do not form one closed traversal by oriented vertex
    /// identity, or an anchored pole use is not at the end of its coedge.
    BoundaryRingChainFailed,
    /// The ring is not continuous as listed and its coedges form more than
    /// one closed traversal.
    BoundaryRingChainAmbiguous,
    /// No pcurve, no edge curve, no use-curve, and the coedge is not
    /// degenerate.
    BoundaryCoedgeUnresolved,
    /// The 3D interval of the edge cannot be established.
    BoundaryEdgeRangeUnresolved,
    /// Two candidate pcurve intervals qualify with different images and no
    /// 3D witness decides.
    BoundaryPcurveIntervalAmbiguous,
    /// A closed pcurve-only coedge has no direction where its direction
    /// matters.
    BoundaryPcurveDirectionAmbiguous,
    /// Consecutive pcurve uses of one coedge do not join.
    BoundaryPcurveChainBroken,
    /// The surface image of a coedge's pcurve does not follow its 3D curve
    /// once, in order, within the allowance.
    BoundaryPcurveCurveMismatch,
    /// A boundary point does not invert onto the surface within the
    /// allowance.
    BoundaryInversionFailed,
    /// A 3D-only coedge on a carrier with no inverse.
    BoundaryInversionUnsupported,
    /// Consecutive pieces do not join within the allowance.
    BoundaryNotClosed,
    /// A loop's closing shift is not a whole number of periods.
    BoundaryPeriodMismatch,
    /// A ring loop with zero net shift encloses no area relative to its own
    /// size.
    BoundaryLoopDegenerate,
    /// The loops form no valid region for the domain.
    BoundaryNestingInvalid,
    /// The document admits two regions.
    BoundaryRegionAmbiguous,
    /// The carrier's domain has no region rule in this build.
    RegionDomainUnsupported,
    /// A witness contradicts the declared orientation.
    RegionOrientationConflict,
    /// An evaluator has no value at a needed parameter.
    EvaluationNoValue,
    /// An evaluation or a result left the finite range.
    EvaluationNonFinite,
    /// The item's work slice, a sampling cap, or the pass allowance ran out.
    EvaluationWorkExhausted,
    /// The quadrature error estimate exceeds the acceptance limit.
    QuadratureNotConverged,
    /// A shell is not closed, or a face reaches an uncovered natural domain
    /// edge.
    VolumeShellOpen,
    /// A face's sense disagrees with its loop winding or its witness, or a
    /// shell's volume sign contradicts its role.
    VolumeOrientationConflict,
    /// A declared-orientation face of the body is unconfirmed.
    VolumeOrientationUnverified,
    /// The flux of the unit field exceeds the closure tolerance.
    VolumeClosureFlux,
    /// A sum or union has an unavailable member.
    AggregateMemberUnavailable,
    /// Torus and elliptic-cone sweeps.
    SweepUnsupportedCarrier,
    /// No interior bound for the carrier.
    BboxInteriorUnbounded,
    /// The NURBS control hull needs positive weights.
    BboxNonpositiveWeights,
    /// A cacheless procedural curve with no pcurve route.
    LengthDerivativeUnavailable,
    /// The surface is used by faces of bodies with different placements.
    PlacementCarrierShared,
    /// A principal moment agrees with another within their separation
    /// tolerance, so its principal axis is not determined.
    MassPrincipalAxesDegenerate,
    /// An occurrence of the chain has a scale other than `(1, 1, 1)`.
    PlacementOccurrenceScale,
    /// An occurrence of the chain or of its subtree states no placement.
    PlacementOccurrenceUnplaced,
    /// The occurrence's prototype is external or unresolved.
    PlacementPrototypeUnresolved,
    /// The occurrence's parent chain does not reach a root occurrence.
    PlacementOccurrenceChainBroken,
    /// Two occurrences of the subtree place the same body.
    PlacementOccurrenceBodyRepeated,
    /// Neither the occurrence's prototype nor a prototype of its subtree
    /// lists a body.
    PlacementOccurrenceEmpty,
    /// A loop's parameter values are so large that a shift by one period
    /// does not change them.
    BoundaryPeriodUnresolved,
}

impl UnavailableReason {
    /// Every reason, in declaration order.
    pub const ALL: [Self; 48] = [
        Self::CarrierUnknown,
        Self::CarrierUnresolved,
        Self::CarrierProceduralUnevaluated,
        Self::CarrierPolygonal,
        Self::CarrierCompositeCurve,
        Self::CarrierEllipticCone,
        Self::PlacementNonSimilarity,
        Self::PlacementNonFinite,
        Self::BoundaryUnboundedFace,
        Self::BoundaryRingChainFailed,
        Self::BoundaryRingChainAmbiguous,
        Self::BoundaryCoedgeUnresolved,
        Self::BoundaryEdgeRangeUnresolved,
        Self::BoundaryPcurveIntervalAmbiguous,
        Self::BoundaryPcurveDirectionAmbiguous,
        Self::BoundaryPcurveChainBroken,
        Self::BoundaryPcurveCurveMismatch,
        Self::BoundaryInversionFailed,
        Self::BoundaryInversionUnsupported,
        Self::BoundaryNotClosed,
        Self::BoundaryPeriodMismatch,
        Self::BoundaryLoopDegenerate,
        Self::BoundaryNestingInvalid,
        Self::BoundaryRegionAmbiguous,
        Self::RegionDomainUnsupported,
        Self::RegionOrientationConflict,
        Self::EvaluationNoValue,
        Self::EvaluationNonFinite,
        Self::EvaluationWorkExhausted,
        Self::QuadratureNotConverged,
        Self::VolumeShellOpen,
        Self::VolumeOrientationConflict,
        Self::VolumeOrientationUnverified,
        Self::VolumeClosureFlux,
        Self::AggregateMemberUnavailable,
        Self::SweepUnsupportedCarrier,
        Self::BboxInteriorUnbounded,
        Self::BboxNonpositiveWeights,
        Self::LengthDerivativeUnavailable,
        Self::PlacementCarrierShared,
        Self::MassPrincipalAxesDegenerate,
        Self::PlacementOccurrenceScale,
        Self::PlacementOccurrenceUnplaced,
        Self::PlacementPrototypeUnresolved,
        Self::PlacementOccurrenceChainBroken,
        Self::PlacementOccurrenceBodyRepeated,
        Self::PlacementOccurrenceEmpty,
        Self::BoundaryPeriodUnresolved,
    ];

    /// The pinned `family.detail` string.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::CarrierUnknown => "carrier.unknown",
            Self::CarrierUnresolved => "carrier.unresolved",
            Self::CarrierProceduralUnevaluated => "carrier.procedural-unevaluated",
            Self::CarrierPolygonal => "carrier.polygonal",
            Self::CarrierCompositeCurve => "carrier.composite-curve",
            Self::CarrierEllipticCone => "carrier.elliptic-cone",
            Self::PlacementNonSimilarity => "placement.non-similarity",
            Self::PlacementNonFinite => "placement.non-finite",
            Self::BoundaryUnboundedFace => "boundary.unbounded-face",
            Self::BoundaryRingChainFailed => "boundary.ring-chain-failed",
            Self::BoundaryRingChainAmbiguous => "boundary.ring-chain-ambiguous",
            Self::BoundaryCoedgeUnresolved => "boundary.coedge-unresolved",
            Self::BoundaryEdgeRangeUnresolved => "boundary.edge-range-unresolved",
            Self::BoundaryPcurveIntervalAmbiguous => "boundary.pcurve-interval-ambiguous",
            Self::BoundaryPcurveDirectionAmbiguous => "boundary.pcurve-direction-ambiguous",
            Self::BoundaryPcurveChainBroken => "boundary.pcurve-chain-broken",
            Self::BoundaryPcurveCurveMismatch => "boundary.pcurve-curve-mismatch",
            Self::BoundaryInversionFailed => "boundary.inversion-failed",
            Self::BoundaryInversionUnsupported => "boundary.inversion-unsupported",
            Self::BoundaryNotClosed => "boundary.not-closed",
            Self::BoundaryPeriodMismatch => "boundary.period-mismatch",
            Self::BoundaryLoopDegenerate => "boundary.loop-degenerate",
            Self::BoundaryNestingInvalid => "boundary.nesting-invalid",
            Self::BoundaryRegionAmbiguous => "boundary.region-ambiguous",
            Self::RegionDomainUnsupported => "region.domain-unsupported",
            Self::RegionOrientationConflict => "region.orientation-conflict",
            Self::EvaluationNoValue => "evaluation.no-value",
            Self::EvaluationNonFinite => "evaluation.non-finite",
            Self::EvaluationWorkExhausted => "evaluation.work-exhausted",
            Self::QuadratureNotConverged => "quadrature.not-converged",
            Self::VolumeShellOpen => "volume.shell-open",
            Self::VolumeOrientationConflict => "volume.orientation-conflict",
            Self::VolumeOrientationUnverified => "volume.orientation-unverified",
            Self::VolumeClosureFlux => "volume.closure-flux",
            Self::AggregateMemberUnavailable => "aggregate.member-unavailable",
            Self::SweepUnsupportedCarrier => "sweep.unsupported-carrier",
            Self::BboxInteriorUnbounded => "bbox.interior-unbounded",
            Self::BboxNonpositiveWeights => "bbox.nonpositive-weights",
            Self::LengthDerivativeUnavailable => "length.derivative-unavailable",
            Self::PlacementCarrierShared => "placement.carrier-shared",
            Self::MassPrincipalAxesDegenerate => "mass.principal-axes-degenerate",
            Self::PlacementOccurrenceScale => "placement.occurrence-scale",
            Self::PlacementOccurrenceUnplaced => "placement.occurrence-unplaced",
            Self::PlacementPrototypeUnresolved => "placement.prototype-unresolved",
            Self::PlacementOccurrenceChainBroken => "placement.occurrence-chain-broken",
            Self::PlacementOccurrenceBodyRepeated => "placement.occurrence-body-repeated",
            Self::PlacementOccurrenceEmpty => "placement.occurrence-empty",
            Self::BoundaryPeriodUnresolved => "boundary.period-unresolved",
        }
    }

    /// The fixed category of this reason. `None` only for
    /// [`Self::AggregateMemberUnavailable`], whose category is the category of
    /// the member value it cites.
    #[must_use]
    pub const fn category(self) -> Option<Category> {
        Some(match self {
            Self::CarrierUnknown
            | Self::CarrierUnresolved
            | Self::BoundaryRingChainFailed
            | Self::BoundaryRingChainAmbiguous
            | Self::BoundaryCoedgeUnresolved
            | Self::BoundaryEdgeRangeUnresolved
            | Self::BoundaryPcurveIntervalAmbiguous
            | Self::BoundaryPcurveDirectionAmbiguous
            | Self::BoundaryPcurveChainBroken
            | Self::BoundaryPcurveCurveMismatch
            | Self::BoundaryInversionFailed
            | Self::BoundaryNotClosed
            | Self::BoundaryPeriodMismatch
            | Self::BoundaryLoopDegenerate
            | Self::BoundaryNestingInvalid
            | Self::BoundaryRegionAmbiguous
            | Self::RegionOrientationConflict
            | Self::EvaluationNoValue
            | Self::EvaluationNonFinite
            | Self::VolumeOrientationConflict
            | Self::VolumeClosureFlux
            | Self::PlacementOccurrenceChainBroken
            | Self::BoundaryPeriodUnresolved => Category::Input,
            Self::BoundaryUnboundedFace
            | Self::VolumeShellOpen
            | Self::MassPrincipalAxesDegenerate
            | Self::PlacementOccurrenceEmpty => Category::Undefined,
            Self::CarrierProceduralUnevaluated
            | Self::CarrierPolygonal
            | Self::CarrierCompositeCurve
            | Self::CarrierEllipticCone
            | Self::BoundaryInversionUnsupported
            | Self::RegionDomainUnsupported
            | Self::VolumeOrientationUnverified
            | Self::SweepUnsupportedCarrier
            | Self::BboxInteriorUnbounded
            | Self::BboxNonpositiveWeights
            | Self::LengthDerivativeUnavailable => Category::Unsupported,
            Self::EvaluationWorkExhausted | Self::QuadratureNotConverged => Category::Budget,
            Self::PlacementNonSimilarity
            | Self::PlacementNonFinite
            | Self::PlacementCarrierShared
            | Self::PlacementOccurrenceScale
            | Self::PlacementOccurrenceUnplaced
            | Self::PlacementPrototypeUnresolved
            | Self::PlacementOccurrenceBodyRepeated => Category::Placement,
            Self::AggregateMemberUnavailable => return None,
        })
    }
}

impl Serialize for UnavailableReason {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

/// A fact about a measured result that a reader should see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaveatCode {
    /// A ring's listed coedge order is not its traversal order.
    BoundaryRingOrder,
    /// A coedge's pcurve uses are listed from its end to its start.
    BoundaryPcurveOrder,
    /// The face's region and normal rest on its declared orientation and no
    /// witness confirms it.
    OrientationUnconfirmed,
    /// Neutral CADIR without a sidecar; every exactness is `not_recorded`.
    MeasureProvenanceNotRecorded,
    /// The decode report states that no geometry was transferred.
    MeasureGeometryNotTransferred,
}

impl CaveatCode {
    /// Every caveat, in declaration order.
    pub const ALL: [Self; 5] = [
        Self::BoundaryRingOrder,
        Self::BoundaryPcurveOrder,
        Self::OrientationUnconfirmed,
        Self::MeasureProvenanceNotRecorded,
        Self::MeasureGeometryNotTransferred,
    ];

    /// The pinned `family.detail` string.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::BoundaryRingOrder => "boundary.ring-order",
            Self::BoundaryPcurveOrder => "boundary.pcurve-order",
            Self::OrientationUnconfirmed => "orientation.unconfirmed",
            Self::MeasureProvenanceNotRecorded => "measure.provenance-not-recorded",
            Self::MeasureGeometryNotTransferred => "measure.geometry-not-transferred",
        }
    }

    /// The fixed category of this caveat.
    #[must_use]
    pub const fn category(self) -> Category {
        match self {
            Self::BoundaryRingOrder
            | Self::BoundaryPcurveOrder
            | Self::MeasureGeometryNotTransferred => Category::Input,
            Self::OrientationUnconfirmed => Category::Unsupported,
            Self::MeasureProvenanceNotRecorded => Category::Undefined,
        }
    }
}

impl Serialize for CaveatCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

/// Why an ID-free comparison is inconclusive or a row is not compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompareCause {
    /// A document's occurrences place bodies non-trivially.
    OccurrencePlacement,
    /// A quantity is unavailable for a `budget`, `unsupported` or
    /// `placement` reason on at least one side and has no comparable
    /// candidates.
    Unavailable,
    /// Bodies whose compatibility is not transitive.
    AmbiguousClass,
    /// A body with no comparable quantity.
    UnmeasuredBody,
    /// No numeric or signature row was compared.
    NothingCompared,
    /// Neither box of a containment row has a lower bound.
    NoLowerBound,
    /// A signature pair has no vertex on either side, so no compared row
    /// depends on the bodies' positions.
    SignatureUnplaced,
}

impl CompareCause {
    /// Every cause, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::OccurrencePlacement,
        Self::Unavailable,
        Self::AmbiguousClass,
        Self::UnmeasuredBody,
        Self::NothingCompared,
        Self::NoLowerBound,
        Self::SignatureUnplaced,
    ];

    /// The pinned `family.detail` string.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::OccurrencePlacement => "compare.occurrence-placement",
            Self::Unavailable => "compare.unavailable",
            Self::AmbiguousClass => "compare.ambiguous-class",
            Self::UnmeasuredBody => "compare.unmeasured-body",
            Self::NothingCompared => "compare.nothing-compared",
            Self::NoLowerBound => "compare.no-lower-bound",
            Self::SignatureUnplaced => "compare.signature-unplaced",
        }
    }
}

impl Serialize for CompareCause {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::{Category, CaveatCode, CompareCause, UnavailableReason};

    const REASONS: [(&str, Option<&str>); 48] = [
        ("carrier.unknown", Some("input")),
        ("carrier.unresolved", Some("input")),
        ("carrier.procedural-unevaluated", Some("unsupported")),
        ("carrier.polygonal", Some("unsupported")),
        ("carrier.composite-curve", Some("unsupported")),
        ("carrier.elliptic-cone", Some("unsupported")),
        ("placement.non-similarity", Some("placement")),
        ("placement.non-finite", Some("placement")),
        ("boundary.unbounded-face", Some("undefined")),
        ("boundary.ring-chain-failed", Some("input")),
        ("boundary.ring-chain-ambiguous", Some("input")),
        ("boundary.coedge-unresolved", Some("input")),
        ("boundary.edge-range-unresolved", Some("input")),
        ("boundary.pcurve-interval-ambiguous", Some("input")),
        ("boundary.pcurve-direction-ambiguous", Some("input")),
        ("boundary.pcurve-chain-broken", Some("input")),
        ("boundary.pcurve-curve-mismatch", Some("input")),
        ("boundary.inversion-failed", Some("input")),
        ("boundary.inversion-unsupported", Some("unsupported")),
        ("boundary.not-closed", Some("input")),
        ("boundary.period-mismatch", Some("input")),
        ("boundary.loop-degenerate", Some("input")),
        ("boundary.nesting-invalid", Some("input")),
        ("boundary.region-ambiguous", Some("input")),
        ("region.domain-unsupported", Some("unsupported")),
        ("region.orientation-conflict", Some("input")),
        ("evaluation.no-value", Some("input")),
        ("evaluation.non-finite", Some("input")),
        ("evaluation.work-exhausted", Some("budget")),
        ("quadrature.not-converged", Some("budget")),
        ("volume.shell-open", Some("undefined")),
        ("volume.orientation-conflict", Some("input")),
        ("volume.orientation-unverified", Some("unsupported")),
        ("volume.closure-flux", Some("input")),
        ("aggregate.member-unavailable", None),
        ("sweep.unsupported-carrier", Some("unsupported")),
        ("bbox.interior-unbounded", Some("unsupported")),
        ("bbox.nonpositive-weights", Some("unsupported")),
        ("length.derivative-unavailable", Some("unsupported")),
        ("placement.carrier-shared", Some("placement")),
        ("mass.principal-axes-degenerate", Some("undefined")),
        ("placement.occurrence-scale", Some("placement")),
        ("placement.occurrence-unplaced", Some("placement")),
        ("placement.prototype-unresolved", Some("placement")),
        ("placement.occurrence-chain-broken", Some("input")),
        ("placement.occurrence-body-repeated", Some("placement")),
        ("placement.occurrence-empty", Some("undefined")),
        ("boundary.period-unresolved", Some("input")),
    ];

    #[test]
    fn reason_codes_are_pinned() {
        for (reason, (code, category)) in UnavailableReason::ALL.into_iter().zip(REASONS) {
            assert_eq!(reason.code(), code);
            assert_eq!(reason.category().map(Category::as_str), category, "{code}");
        }
    }

    #[test]
    fn reason_codes_are_unique() {
        let mut codes: Vec<&str> = UnavailableReason::ALL
            .iter()
            .map(|reason| reason.code())
            .collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), UnavailableReason::ALL.len());
        let mut caveats: Vec<&str> = CaveatCode::ALL.iter().map(|code| code.code()).collect();
        caveats.sort_unstable();
        caveats.dedup();
        assert_eq!(caveats.len(), CaveatCode::ALL.len());
    }

    #[test]
    fn reason_codes_are_family_dot_detail() {
        let codes = UnavailableReason::ALL
            .iter()
            .map(|reason| reason.code())
            .chain(CaveatCode::ALL.iter().map(|code| code.code()))
            .chain(CompareCause::ALL.iter().map(|cause| cause.code()));
        for code in codes {
            let (family, detail) = code.split_once('.').expect("family.detail");
            assert!(!family.is_empty() && !detail.is_empty(), "{code}");
            assert!(!detail.contains('.'), "{code}");
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c == '.'),
                "{code}"
            );
        }
    }

    #[test]
    fn caveat_codes_are_pinned() {
        let expected = [
            ("boundary.ring-order", "input"),
            ("boundary.pcurve-order", "input"),
            ("orientation.unconfirmed", "unsupported"),
            ("measure.provenance-not-recorded", "undefined"),
            ("measure.geometry-not-transferred", "input"),
        ];
        for (code, (text, category)) in CaveatCode::ALL.into_iter().zip(expected) {
            assert_eq!(code.code(), text);
            assert_eq!(code.category().as_str(), category);
        }
    }

    #[test]
    fn compare_causes_are_pinned() {
        let expected = [
            "compare.occurrence-placement",
            "compare.unavailable",
            "compare.ambiguous-class",
            "compare.unmeasured-body",
            "compare.nothing-compared",
            "compare.no-lower-bound",
            "compare.signature-unplaced",
        ];
        for (cause, text) in CompareCause::ALL.into_iter().zip(expected) {
            assert_eq!(cause.code(), text);
        }
    }

    /// The rows of the first Markdown table after `heading` whose first cell
    /// is a code in backticks.
    fn contract_rows(contract: &str, heading: &str) -> Vec<Vec<String>> {
        let start = contract.find(heading).expect("contract heading");
        contract[start..]
            .lines()
            .skip(1)
            .skip_while(|line| !line.starts_with('|'))
            .take_while(|line| line.starts_with('|'))
            .filter(|line| line.starts_with("| `"))
            .map(|line| {
                line.trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().trim_matches('`').to_owned())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn reason_table_matches_the_measurement_contract() {
        let contract = include_str!("../../../docs/measurement.md");
        let reasons = contract_rows(contract, "## Unavailable reasons");
        assert_eq!(reasons.len(), UnavailableReason::ALL.len());
        for (row, reason) in reasons.iter().zip(UnavailableReason::ALL) {
            assert_eq!(row[0], reason.code());
            let category = reason
                .category()
                .map_or("from the member", Category::as_str);
            assert_eq!(row[1], category, "{}", row[0]);
        }
        let caveats = contract_rows(contract, "## Caveats");
        assert_eq!(caveats.len(), CaveatCode::ALL.len());
        for (row, caveat) in caveats.iter().zip(CaveatCode::ALL) {
            assert_eq!(row[0], caveat.code());
            assert_eq!(row[1], caveat.category().as_str(), "{}", row[0]);
        }
        let causes = contract_rows(contract, "## Comparison causes");
        assert_eq!(causes.len(), CompareCause::ALL.len());
        for (row, cause) in causes.iter().zip(CompareCause::ALL) {
            assert_eq!(row[0], cause.code());
        }
    }
}
