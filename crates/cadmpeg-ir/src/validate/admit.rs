// SPDX-License-Identifier: Apache-2.0
//! Decoder admission routes: the named rules whose Error findings refuse a
//! candidate reading.
//!
//! A decoder may test a candidate reading of a part against part 10's rules
//! on model-class records and give the part up where the reading fails them
//! (§3.3). Each route lists those rules by name, so a rule added to the
//! validator later refuses no candidate until a route lists it.

pub(crate) mod incremental;

use crate::annotations::Annotations;
use crate::document::CadIr;
use crate::report::{
    check::{RuleName, ValidationReport},
    loss::LossNote,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// A decoder admission route: the rules whose Error findings refuse a
/// candidate, and whether the route validates annotations.
#[derive(Debug, PartialEq, Eq)]
pub struct Admission {
    rules: &'static [RuleName],
    annotations: bool,
}

impl Admission {
    /// A route that admits a candidate unless it breaks one of `rules`, and
    /// validates annotations where `annotations` holds.
    #[must_use]
    pub const fn new(rules: &'static [RuleName], annotations: bool) -> Self {
        Self { rules, annotations }
    }

    /// Whether a finding of `rule` counts toward this route.
    #[must_use]
    pub fn admits(&self, rule: &RuleName) -> bool {
        self.rules.contains(rule)
    }

    /// The rules of this route.
    #[must_use]
    pub const fn rules(&self) -> &'static [RuleName] {
        self.rules
    }

    /// Whether this route validates annotations.
    #[must_use]
    pub const fn annotations(&self) -> bool {
        self.annotations
    }
}

/// The rules of ids and references, native links, topology, carriers,
/// parameter domains, and geometric consistency on model-class records,
/// with `$extra` after them.
macro_rules! draft_core_rules {
    ($($extra:literal),* $(,)?) => {
        &[
            crate::rule_name!("annotations_native.helix-axis-native-ref-does-not"),
            crate::rule_name!("annotations_native.native-record-link-does-not-resolve"),
            crate::rule_name!("annotations_native.native-record-link-must-identity-string"),
            crate::rule_name!("annotations_native.native-record-links-must-array-identities"),
            crate::rule_name!("annotations_native.native-ref-does-not-resolve"),
            crate::rule_name!("annotations_native.operand-native-ref-does-not-resolve"),
            crate::rule_name!("annotations_native.pmi-native-ref-does-not-resolve"),
            crate::rule_name!("carriers_parameterization.coedge-pcurve-range-outside-carrier-domain"),
            crate::rule_name!("carriers_parameterization.coedge-use-curve-names-no-curve"),
            crate::rule_name!("carriers_parameterization.coedge-use-curve-range-outside-carrier"),
            crate::rule_name!("carriers_parameterization.edge-parameter-range-outside-canonical-carrier"),
            crate::rule_name!("carriers_parameterization.edge-parameter-range-outside-local-domain"),
            crate::rule_name!("carriers_parameterization.orphan-carrier"),
            crate::rule_name!("drawings.drawing-views-base-view-name-record"),
            crate::rule_name!("drawings.invalid-drawing-reference-order-numeric-state"),
            crate::rule_name!("eval.model-carrier-evaluations-reached-local-work"),
            crate::rule_name!("evaluation_cycles.malformed-curve-surface-reference-cycle"),
            crate::rule_name!("geometry_consistency.charted-tolerant-intersection-does-not-evaluate"),
            crate::rule_name!("geometry_consistency.charted-tolerant-intersection-misses-endpoint-witnesses"),
            crate::rule_name!("geometry_consistency.coedge-use-curve-endpoints-miss-traversal"),
            crate::rule_name!("geometry_consistency.edge-curve-endpoints-miss-edge-vertex"),
            crate::rule_name!("geometry_consistency.edge-no-vertex-lies-open-carrier"),
            crate::rule_name!("geometry_consistency.edge-runs-against-curve-start-vertex"),
            crate::rule_name!("geometry_consistency.pcurve-mapped-through-face-surface-misses"),
            crate::rule_name!("geometry_consistency.procedural-support-side-misses-endpoint-distance"),
            crate::rule_name!("geometry_consistency.surface-offset-solved-curve-misses-base"),
            crate::rule_name!("history_bodies.history-body-names-neither-producing-nor"),
            crate::rule_name!("history_bodies.history-body-names-no-feature"),
            crate::rule_name!("history_bodies.history-body-record-repeats-identity-another"),
            crate::rule_name!("history_bodies.history-body-record-repeats-top-level"),
            crate::rule_name!("identity_order.entity-id-does-not-match-format"),
            crate::rule_name!("identity_order.entity-id-not-globally-unique"),
            crate::rule_name!("physics.references-missing"),
            crate::rule_name!("point_clouds.held-external-product-definition"),
            crate::rule_name!("point_clouds.references-missing-product-definition"),
            crate::rule_name!("presentation.invalid-document-presentation-state"),
            crate::rule_name!("presentation.invalid-view-presentation-reference-order-size"),
            crate::rule_name!("presentation.multiple-document-presentation-records"),
            crate::rule_name!("presentation.unresolved-presentation-layer-parent"),
            crate::rule_name!("products.assembly-joint-couples-other-two-other"),
            crate::rule_name!("products.assembly-joint-frame-sense-contradicts-kind"),
            crate::rule_name!("products.definition-generated-itself"),
            crate::rule_name!("products.definition-generated-no-configuration"),
            crate::rule_name!("products.external-definition-holds-bodies-occurrences-scope"),
            crate::rule_name!("products.feature-output-not-owned-product-definition"),
            crate::rule_name!("products.invalid-assembly-joint-driving-parameter"),
            crate::rule_name!("products.invalid-assembly-joint-operands-frames-limits"),
            crate::rule_name!("products.invalid-product-body-reference"),
            crate::rule_name!("products.invalid-product-definition-owner"),
            crate::rule_name!("products.occurrence-owning-definition"),
            crate::rule_name!("products.occurrence-places-root-definition"),
            crate::rule_name!("products.occurrence-prototype-and-owner"),
            crate::rule_name!("products.placement-linear-part-singular"),
            crate::rule_name!("products.product-definition-places-itself"),
            crate::rule_name!("products.representation-applies-substitution-instances-no-member"),
            crate::rule_name!("products.representation-names-no-body-definition"),
            crate::rule_name!("products.representation-names-no-feature-definition-scope"),
            crate::rule_name!("products.representation-no-representation"),
            crate::rule_name!("products.representation-replaces-member-member-own-path"),
            crate::rule_name!("products.representation-replaces-member-no-occurrence"),
            crate::rule_name!("products.sketch-owning-definition"),
            crate::rule_name!("referential_integrity.entity-schema-cannot-state-typed-references"),
            crate::rule_name!("referential_integrity.unresolved-typed-reference"),
            crate::rule_name!("semantic_annotations.invalid-semantic-annotation-reference-order-numeric"),
            crate::rule_name!("sketches.constraint-entity-kinds"),
            crate::rule_name!("sketches.diameter-parameter-does-not-equal-twice"),
            crate::rule_name!("sketches.invalid-spatial-sketch-nurbs-surface"),
            crate::rule_name!("sketches.point-object-support-itself-point"),
            crate::rule_name!("sketches.projected-copy-entities-do-not-identical"),
            crate::rule_name!("sketches.repeated-spatial-distance-requires-disjoint-parallel"),
            crate::rule_name!("sketches.repeated-spatial-line-length-requires-distinct"),
            crate::rule_name!("sketches.single-entity-spatial-sketch-profile-not"),
            crate::rule_name!("sketches.sketch-constraint-locus-incompatible-entity"),
            crate::rule_name!("sketches.sketch-offset-pair-does-not-match"),
            crate::rule_name!("sketches.sketch-profile-disconnected-consecutive-entities"),
            crate::rule_name!("sketches.spatial-coincidence-requires-two-points"),
            crate::rule_name!("sketches.spatial-constraint-member-does-not-belong"),
            crate::rule_name!("sketches.spatial-constraint-references-missing-spatial-sketch"),
            crate::rule_name!("sketches.spatial-directional-constraint-requires-line"),
            crate::rule_name!("sketches.spatial-distance-requires-parallel-lines-separated"),
            crate::rule_name!("sketches.spatial-line-length-requires-line-matching"),
            crate::rule_name!("sketches.spatial-line-not-parallel-constraint-direction"),
            crate::rule_name!("sketches.spatial-midpoint-requires-point-line"),
            crate::rule_name!("sketches.spatial-offset-distance-does-not-match"),
            crate::rule_name!("sketches.spatial-offset-source-result-members-must"),
            crate::rule_name!("sketches.spatial-parallel-line-set-distance-requires"),
            crate::rule_name!("sketches.spatial-point-distance-requires-two-points"),
            crate::rule_name!("sketches.spatial-point-line-distance-requires-point"),
            crate::rule_name!("sketches.spatial-point-line-distance-requires-point-line"),
            crate::rule_name!("sketches.spatial-point-surface-requires-point-surface"),
            crate::rule_name!("sketches.spatial-sketch-entity-references-missing-spatial"),
            crate::rule_name!("sketches.spatial-sketch-profile-disconnected-consecutive-entities"),
            crate::rule_name!("sketches.spatial-sketch-profile-entity-does-not"),
            crate::rule_name!("sketches.spatial-symmetry-requires-two-points-reflected"),
            crate::rule_name!("sketches.spatial-tangent-requires-two-curves"),
            crate::rule_name!("spreadsheets.spreadsheet-cell-different-owner"),
            crate::rule_name!("spreadsheets.spreadsheet-cell-does-not-resolve"),
            crate::rule_name!("spreadsheets.spreadsheet-feature-does-not-resolve"),
            crate::rule_name!("topology.active-configuration-suppression-disagrees-current-feature"),
            crate::rule_name!("topology.attached-plate-profile-not-planar"),
            crate::rule_name!("topology.attached-plate-requires-exactly-one-bend"),
            crate::rule_name!("topology.axisrotate-must-bind-axis-component"),
            crate::rule_name!("topology.bend-attached-plate-must-precede-bend"),
            crate::rule_name!("topology.binder-target-feature-does-not-precede"),
            crate::rule_name!("topology.catmull-clark-cage-edges-state-different"),
            crate::rule_name!("topology.catmull-clark-face-side-holds-more"),
            crate::rule_name!("topology.child-parent-lists"),
            crate::rule_name!("topology.component-transfer-placement-contains-non-finite"),
            crate::rule_name!("topology.composite-curve-graph-contains-cycle"),
            crate::rule_name!("topology.configuration-definition-feature-does-not-precede"),
            crate::rule_name!("topology.configuration-feature-dependency-does-not-precede"),
            crate::rule_name!("topology.configuration-feature-state-omits-referenced-feature"),
            crate::rule_name!("topology.configuration-parameter-value-invalid"),
            crate::rule_name!("topology.configuration-state-closure-uses-suppressed-dependency"),
            crate::rule_name!("topology.construction-arc-endpoint-outside-radius-plane"),
            crate::rule_name!("topology.construction-frame-must-orthonormal-right-handed"),
            crate::rule_name!("topology.construction-no-input-step-resolved-value"),
            crate::rule_name!("topology.construction-plane-axes-must-perpendicular"),
            crate::rule_name!("topology.construction-relation-does-not-admit-reversed"),
            crate::rule_name!("topology.construction-relation-incompatible-operand-count"),
            crate::rule_name!("topology.construction-relation-incompatible-scalar-value"),
            crate::rule_name!("topology.construction-solution-must-positive"),
            crate::rule_name!("topology.construction-subject-not-part-result-kind"),
            crate::rule_name!("topology.content-child-does-not-follow-parent"),
            crate::rule_name!("topology.content-parameter-belongs-another-feature"),
            crate::rule_name!("topology.coordinates-requires-frame-reference"),
            crate::rule_name!("topology.corner-bend-must-precede-corner-occur"),
            crate::rule_name!("topology.datum-axis"),
            crate::rule_name!("topology.datum-axis-datum-part-incompatible-selected"),
            crate::rule_name!("topology.datum-axis-datum-point-part-remains"),
            crate::rule_name!("topology.datum-axis-datum-selection-must-target"),
            crate::rule_name!("topology.datum-axis-dimension-binds-parameter-incompatible"),
            crate::rule_name!("topology.datum-axis-dimension-must-bind-parameter"),
            crate::rule_name!("topology.datum-axis-dimension-target-must-name"),
            crate::rule_name!("topology.datum-axis-feature-input-must-precede"),
            crate::rule_name!("topology.datum-axis-parameter-owner-must-precede"),
            crate::rule_name!("topology.datum-axis-region-boundary-must-belong"),
            crate::rule_name!("topology.datum-axis-target-no-declared-arena"),
            crate::rule_name!("topology.datum-plane-feature-reference-does-not"),
            crate::rule_name!("topology.datum-plane-reference-cycle-contains"),
            crate::rule_name!("topology.datum-point-omits-plane-feature-dependencies"),
            crate::rule_name!("topology.datum-point-plane-does-not-precede"),
            crate::rule_name!("topology.datum-point-plane-reference-does-not"),
            crate::rule_name!("topology.datum-point-sketch-point-selection-invalid"),
            crate::rule_name!("topology.dependency-feature-does-not-precede-consumer"),
            crate::rule_name!("topology.derived-geometry-omits-source-feature-dependencies"),
            crate::rule_name!("topology.does-not-precede-consuming-feature"),
            crate::rule_name!("topology.draft-pull-plane-frame-normal-not"),
            crate::rule_name!("topology.draft-pull-states-both-datum-plane"),
            crate::rule_name!("topology.edge-no-vertex-must-carrier-curve"),
            crate::rule_name!("topology.face-lists-loop-more-once"),
            crate::rule_name!("topology.feature-omits-dependency"),
            crate::rule_name!("topology.feature-reference-does-not-name-datum"),
            crate::rule_name!("topology.fixed-construction-cannot-carry-resolved-values"),
            crate::rule_name!("topology.fold-requires-four-picked-edges"),
            crate::rule_name!("topology.fold-requires-whole-plane-frame"),
            crate::rule_name!("topology.free-vertex-also-referenced-edge"),
            crate::rule_name!("topology.free-vertex-must-belong-exactly-one"),
            crate::rule_name!("topology.generated-body-selection-invalid"),
            crate::rule_name!("topology.generated-edge-selection-invalid"),
            crate::rule_name!("topology.generated-face-selection-invalid"),
            crate::rule_name!("topology.generated-profile-curve-invalid"),
            crate::rule_name!("topology.generated-vertex-invalid"),
            crate::rule_name!("topology.historical-selection-uses-another-feature-input"),
            crate::rule_name!("topology.lists-parent-another-record"),
            crate::rule_name!("topology.literal-false-flip-redundant"),
            crate::rule_name!("topology.not-listed"),
            crate::rule_name!("topology.offset-plane-omits-reference-feature-dependencies"),
            crate::rule_name!("topology.owner-does-not-precede-profile-consumer"),
            crate::rule_name!("topology.parameter-dependency-does-not-precede-consumer"),
            crate::rule_name!("topology.pattern-omits-seed-feature-dependencies"),
            crate::rule_name!("topology.plane-equation-normal-cannot-zero"),
            crate::rule_name!("topology.point-operand-cannot-supply-construction-direction"),
            crate::rule_name!("topology.profile-feature-not-preceding-dependency"),
            crate::rule_name!("topology.radial-ring-crosses-edges"),
            crate::rule_name!("topology.radial-ring-does-not-close"),
            crate::rule_name!("topology.radius-limit-require-curve-construction"),
            crate::rule_name!("topology.reference-plane-does-not-precede-offset"),
            crate::rule_name!("topology.references-missing"),
            crate::rule_name!("topology.refold-unfold-must-precede-refold-occur"),
            crate::rule_name!("topology.region-mark-names-shell-twice-one"),
            crate::rule_name!("topology.revolution-angular-travel-must-not-exceed"),
            crate::rule_name!("topology.scale-transform-invalid"),
            crate::rule_name!("topology.seed-feature-does-not-precede-pattern"),
            crate::rule_name!("topology.selection-rule-incompatible-seed-count"),
            crate::rule_name!("topology.sheet-metal-edge-flange-height-invalid"),
            crate::rule_name!("topology.shell-faces-disconnected-through-shared-edges"),
            crate::rule_name!("topology.shell-lists-member-more-once"),
            crate::rule_name!("topology.single-explicit-selection-term-must-use"),
            crate::rule_name!("topology.sketch-block-does-not-precede-instance"),
            crate::rule_name!("topology.sketch-block-instance-omits-block-feature"),
            crate::rule_name!("topology.sketch-block-target-not-block-definition"),
            crate::rule_name!("topology.sketch-entity-belongs-different-sketch"),
            crate::rule_name!("topology.sketch-multiple-owning-features"),
            crate::rule_name!("topology.sketch-owner-does-not-precede-datum"),
            crate::rule_name!("topology.sketch-owner-does-not-precede-path"),
            crate::rule_name!("topology.sketch-path-curves-empty-repeated-owned"),
            crate::rule_name!("topology.sketch-profile-entities-empty-repeated-missing"),
            crate::rule_name!("topology.sketch-profile-indices-empty-repeated-out"),
            crate::rule_name!("topology.sketch-regions-empty-repeated-invalid-out"),
            crate::rule_name!("topology.source-feature-does-not-precede-derived"),
            crate::rule_name!("topology.spatial-sketch-path-curves-empty-repeated"),
            crate::rule_name!("topology.spatial-sketch-profile-indices-empty-repeated"),
            crate::rule_name!("topology.split-face-direction-input-run-inconsistent"),
            crate::rule_name!("topology.text-dimensions-cannot-negate-parameter"),
            crate::rule_name!("topology.two-member-radial-ring-equal-coedge"),
            crate::rule_name!("topology.two-sided-draft-no-neutral-plane"),
            crate::rule_name!("topology.whole-coincident-frame-requires-frame-result"),
            crate::rule_name!("topology.wire-body-contains-faces"),
            crate::rule_name!("topology.wire-edge-also-referenced-coedge"),
            crate::rule_name!("topology.wire-edge-must-belong-exactly-one"),
            crate::rule_name!("topology.zaxis-constraint-must-act-whole-construction"),
            $(crate::rule_name!($extra)),*
        ]
    };
}

/// The model-class rules a decoder may test a candidate against (§3.3).
/// `arena_order` rules are not in it: a candidate is judged before
/// [`CadIr::finalize`](crate::CadIr::finalize) orders its arenas.
pub const DRAFT_CORE: &Admission = &Admission::new(DRAFT_CORE_RULES, false);

const DRAFT_CORE_RULES: &[RuleName] = draft_core_rules!();

/// Rhino draft candidates: [`DRAFT_CORE`] and the rules on annotations.
pub const RHINO_DRAFT: &Admission = &Admission::new(RHINO_DRAFT_RULES, true);

const RHINO_DRAFT_RULES: &[RuleName] = draft_core_rules!(
    "annotations_native.entity-could-not-serialized-validate-exactness",
    "annotations_native.exactness-field-path-does-not-resolve",
    "annotations_native.exactness-key-does-not-resolve-entity",
    "annotations_native.provenance-key-does-not-resolve-entity",
);

/// Rhino instance expansion: [`DRAFT_CORE`]. Mid-expansion candidates are
/// not finalized.
pub const RHINO_INSTANCE: &Admission = DRAFT_CORE;

/// CATIA topology admission after
/// [`Model::finalize`](crate::document::Model::finalize); pending native
/// identities are supplied through
/// [`admit_with_additional_native_identities`].
pub const CATIA_ADMISSION: &Admission = DRAFT_CORE;

/// Drop the findings whose rule `allowed` does not list.
fn filter_rules(mut report: ValidationReport, allowed: &Admission) -> ValidationReport {
    report
        .findings
        .retain(|finding| allowed.admits(&finding.rule));
    report
}

/// Validate under the caller's live session, then retain findings in `allowed`.
pub fn admit(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    allowed: &Admission,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    Ok(filter_rules(
        super::validate_model(ctx, ir, losses)?,
        allowed,
    ))
}

/// Admit with borrowed annotations under the caller's live session.
pub fn admit_with_annotations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &Annotations,
    allowed: &Admission,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    Ok(filter_rules(
        super::validate_model_with_annotations(ctx, ir, annotations, losses)?,
        allowed,
    ))
}

/// Admit while treating staged native identities as resolvable in scoped indexes.
pub fn admit_with_additional_native_identities<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    additional: impl IntoIterator<Item = &'a str>,
    allowed: &Admission,
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    let index = crate::index::ModelIndex::with_additional_native_identities(ir, additional);
    Ok(filter_rules(
        super::validate_model_with_index(ctx, ir, losses, &index)?,
        allowed,
    ))
}

/// Admit source records through a borrowed replacement of one native unknown arena.
/// Resource refusals remain outside source-product validation failures.
pub fn admit_with_native_unknowns(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    (format, records): (&str, &[crate::unknown::UnknownRecord]),
    annotations: Option<&Annotations>,
    allowed: &Admission,
    losses: Vec<LossNote>,
) -> Result<Result<ValidationReport, crate::native::NativeConvertError>, CodecError> {
    let positions = match native_unknown_order(records) {
        Ok(positions) => positions,
        Err(error) => return Ok(Err(error)),
    };
    let index = crate::index::ModelIndex::with_native_unknowns(ir, format, records, &positions);
    let mut report = super::validate_model_with_index(ctx, ir, losses, &index)?;
    if let Some(annotations) = annotations {
        super::validate_annotations(
            ctx,
            &index,
            annotations,
            std::iter::empty(),
            &mut report.findings,
        )?;
    }
    Ok(Ok(filter_rules(report, allowed)))
}

/// Validate a sparse replacement against the combined document with one model index.
pub(crate) fn admit_annotation_delta(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    (format, records): (&str, &[crate::unknown::UnknownRecord]),
    annotations: &crate::annotations::SparseAnnotationTransaction<'_>,
    allowed: &Admission,
) -> Result<Result<ValidationReport, crate::native::NativeConvertError>, CodecError> {
    let positions = match native_unknown_order(records) {
        Ok(positions) => positions,
        Err(error) => return Ok(Err(error)),
    };
    let index = crate::index::ModelIndex::with_native_unknowns(ir, format, records, &positions);
    let mut report = super::validate_model_with_index(ctx, ir, Vec::new(), &index)?;
    let mut old = Vec::new();
    super::validate_annotations(
        ctx,
        &index,
        annotations.base(),
        std::iter::empty(),
        &mut old,
    )?;
    for finding in old {
        if let Some(id) = &finding.entity {
            if annotations.selected(id) {
                continue;
            }
        }
        report.findings.push(finding);
    }
    super::validate_annotations(
        ctx,
        &index,
        annotations.annotations(),
        std::iter::empty(),
        &mut report.findings,
    )?;
    Ok(Ok(filter_rules(report, allowed)))
}

/// Check source-product link grammar and distinct identities without validating the model.
/// Raw images and link text remain borrowed throughout the scoped identity sort.
pub fn validate_native_unknowns(
    records: &[crate::unknown::UnknownRecord],
) -> Result<(), crate::native::NativeConvertError> {
    native_unknown_order(records).map(|_| ())
}

/// Positions of `records` in identity order, or the first grammar or
/// duplicate-identity refusal.
fn native_unknown_order(
    records: &[crate::unknown::UnknownRecord],
) -> Result<Vec<usize>, crate::native::NativeConvertError> {
    native_unknown_grammar(records)?;
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_unstable_by(|left, right| {
        records[*left]
            .id()
            .as_str()
            .cmp(records[*right].id().as_str())
    });
    for pair in order.windows(2) {
        let first = records[pair[0]].id().as_str();
        if first == records[pair[1]].id().as_str() {
            return Err(crate::native::NativeConvertError::InvalidCollection(
                format!("duplicate native unknown record {first}"),
            ));
        }
    }
    Ok(order)
}

fn native_unknown_grammar(
    records: &[crate::unknown::UnknownRecord],
) -> Result<(), crate::native::NativeConvertError> {
    for record in records {
        for (position, link) in record.links().iter().enumerate() {
            if !crate::ids::is_valid_identity(link) {
                return Err(crate::native::NativeConvertError::InvalidCollection(
                    format!(
                        "native unknown {} link {position}: identity is invalid: {link:?}",
                        record.id()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Cached source identity admission. Link edits do not change identity order.
#[derive(Debug)]
pub(crate) struct NativeUnknownAdmission {
    dirty: std::collections::BTreeSet<usize>,
}

impl NativeUnknownAdmission {
    pub(crate) fn build(
        records: &[crate::unknown::UnknownRecord],
    ) -> Result<Self, crate::native::NativeConvertError> {
        native_unknown_order(records)?;
        Ok(Self {
            dirty: std::collections::BTreeSet::new(),
        })
    }

    pub(crate) fn dirty(&mut self, position: usize) {
        self.dirty.insert(position);
    }

    pub(crate) fn check(
        &mut self,
        records: &[crate::unknown::UnknownRecord],
    ) -> Result<Result<(), crate::native::NativeConvertError>, CodecError> {
        for &position in &self.dirty {
            let record = records
                .get(position)
                .ok_or_else(|| CodecError::malformed("changed source record is absent"))?;
            if let Err(error) = native_unknown_grammar(std::slice::from_ref(record)) {
                return Ok(Err(error));
            }
        }
        self.dirty.clear();
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod native_unknown_tests;

#[cfg(test)]
mod tests {
    use super::{admit, Admission, CATIA_ADMISSION, DRAFT_CORE, RHINO_DRAFT, RHINO_INSTANCE};
    use crate::report::check::{Check, Finding, RuleName, ValidationReport};
    use crate::report::Severity;
    use crate::test_support::admissibility::{
        accepted_empty, rejected_missing_point, rejected_missing_region,
    };

    #[test]
    fn draft_core_agrees_with_full_on_freeze_fixtures() {
        let accepted = accepted_empty();
        assert!(super::super::validate_neutral(&accepted, Vec::new())
            .expect("validate the neutral model")
            .is_ok());
        assert!(admit(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &accepted,
            DRAFT_CORE,
            Vec::new()
        )
        .expect("admit the model")
        .is_ok());

        let missing_point = rejected_missing_point("test:model").expect("valid identity");
        assert!(!super::super::validate_neutral(&missing_point, Vec::new())
            .expect("validate the neutral model")
            .is_ok());
        assert!(!admit(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &missing_point,
            DRAFT_CORE,
            Vec::new()
        )
        .expect("admit the model")
        .is_ok());

        let missing_region = rejected_missing_region("test:model").expect("valid identity");
        assert!(!super::super::validate_neutral(&missing_region, Vec::new())
            .expect("validate the neutral model")
            .is_ok());
        assert!(!admit(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &missing_region,
            DRAFT_CORE,
            Vec::new()
        )
        .expect("admit the model")
        .is_ok());
    }

    #[test]
    fn a_route_ignores_findings_of_rules_it_does_not_list() {
        const IDENTITY_ONLY: &[RuleName] = &[crate::rule_name!("test.identity-only")];
        let ir = rejected_missing_point("test:model").expect("valid identity");
        let identity_only = Admission::new(IDENTITY_ONLY, false);
        let filtered = admit(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &ir,
            &identity_only,
            Vec::new(),
        )
        .expect("admit the model");
        assert!(filtered.is_ok(), "{filtered:?}");
    }

    /// A rule added later to a check family of a route refuses no candidate:
    /// the route lists rules, not families.
    #[test]
    fn a_new_rule_of_a_listed_family_refuses_no_candidate() {
        let finding = Finding {
            check: Check::ShellTopology,
            rule: crate::rule_name!("test.added-later"),
            severity: Severity::Error,
            message: "a rule added after the route was written".into(),
            entity: None,
            related: Vec::new(),
        };
        for allowed in [DRAFT_CORE, RHINO_DRAFT, RHINO_INSTANCE, CATIA_ADMISSION] {
            let report = ValidationReport {
                entity_counts: std::collections::BTreeMap::new(),
                findings: vec![finding.clone()],
                losses: Vec::new(),
            };
            assert!(super::filter_rules(report, allowed).is_ok());
        }
    }

    /// The routes list model-class rules of the validator only: no design or
    /// process rule (§3.3) and no codec's own rule.
    #[test]
    fn routes_list_only_model_class_validator_rules() {
        for allowed in [DRAFT_CORE, RHINO_DRAFT, RHINO_INSTANCE, CATIA_ADMISSION] {
            for rule in allowed.rules() {
                let name = rule.as_str();
                assert!(
                    !name.contains('/')
                        && !name.starts_with("design.")
                        && !name.starts_with("process.")
                        && !name.starts_with("test."),
                    "{name}"
                );
            }
        }
        assert_eq!(RHINO_INSTANCE, DRAFT_CORE);
        assert_eq!(CATIA_ADMISSION, DRAFT_CORE);
        assert_eq!(
            &RHINO_DRAFT.rules()[..DRAFT_CORE.rules().len()],
            DRAFT_CORE.rules()
        );
        assert!(RHINO_DRAFT.annotations() && !DRAFT_CORE.annotations());
        let mut names: Vec<&str> = RHINO_DRAFT.rules().iter().map(RuleName::as_str).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            RHINO_DRAFT.rules().len(),
            "a route lists a rule once"
        );

        let accepted = accepted_empty();
        let rejected = rejected_missing_point("test:model").expect("valid identity");
        for allowed in [DRAFT_CORE, RHINO_DRAFT, RHINO_INSTANCE, CATIA_ADMISSION] {
            assert!(admit(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &accepted,
                allowed,
                Vec::new()
            )
            .expect("admit the model")
            .is_ok());
            assert!(!admit(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &rejected,
                allowed,
                Vec::new()
            )
            .expect("admit the model")
            .is_ok());
        }
    }

    #[test]
    fn admission_routes_preserve_live_depth_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let ir = crate::CadIr::empty();
        let annotations = crate::annotations::Annotations::default();
        for route in 0..3 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = match route {
                0 => admit(&ctx, &ir, DRAFT_CORE, Vec::new()),
                1 => {
                    super::admit_with_annotations(&ctx, &ir, &annotations, RHINO_DRAFT, Vec::new())
                }
                _ => super::admit_with_additional_native_identities(
                    &ctx,
                    &ir,
                    std::iter::empty(),
                    DRAFT_CORE,
                    Vec::new(),
                ),
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("live session must refuse admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }
}
