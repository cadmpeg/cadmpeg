// SPDX-License-Identifier: Apache-2.0
//! The rules on selections (§5.8, part 10 "Selections").
//!
//! A pick's target is checked in the definition it resolves in; a pick with
//! a context resolves in another definition, whose order the owner's
//! anchor does not reach, so its order is not checked here. A pick whose
//! target definition has no scope in this document is not checked.

use crate::report::check::Ruled;
use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::design::feature::FeatureOperation;
use crate::design::names::NameKind;
use crate::design::ops::datum::DatumElement;
use crate::design::refs::{EntityRef, Evaluated, Member, Position};
use crate::design::selection::{
    Anchor, DatumPart, DefinitionRef, OccurrencePath, PickContext, PickDomain, PickTarget, Picked,
    SelectionRule, StepRef,
};
use crate::document::CadIr;
use crate::features::ConfigurationId;
use crate::ids::ProductDefinitionId;
use crate::native::view::NativeView;
use crate::report::check::{Check, Finding};
use crate::report::Severity;

use super::configurations::check_configuration_ref;
use super::evaluations::check_resulting_position;
use super::index::{DesignIndex, StateAt};
use super::matching::Matchers;
use super::names::derived_kinds;
use super::records::{DesignRecords, MemberKey, PositionKey};
use super::states::{Extent, States};
use super::walk::{PicksView, SelectionView, TermView};
use super::{record_finding, Held, Holder};

/// Where a pick sits: its holder, its selection's anchor, and the definition
/// it resolves in without a context.
#[derive(Clone, Copy)]
pub(super) struct Site<'a> {
    pub(super) holder: Holder<'a>,
    pub(super) anchor: &'a Anchor,
    pub(super) owner: Option<&'a ProductDefinitionId>,
}

impl<'a> Site<'a> {
    /// The site of a selection that `record` holds, anchored at `anchor`.
    pub(super) fn of(index: &DesignIndex<'a>, record: &Held<'a>, anchor: &'a Anchor) -> Self {
        let owner = match record.holder {
            Holder::Definition(_) | Holder::Owned { .. } => record
                .holder
                .feature()
                .and_then(|feature| index.owner_definition(feature)),
            Holder::Record { scope, .. } => scope,
        };
        Self {
            holder: record.holder,
            anchor,
            owner,
        }
    }
}

/// Checks every selection of the design, and every publication's target.
pub(super) fn check<'a>(
    ctx: &DecodeContext<'_>,
    index: &DesignIndex<'a>,
    matchers: &Matchers<'a>,
    held: &'a [Held<'a>],
    document: NativeView<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let moved = moved_occurrences(index.records.ir);
    for record in held {
        for selection in &record.selections {
            let site = Site::of(index, record, selection.at);
            check_anchor(index, site, selection, findings);
            check_rules(selection, site, findings);
            for pick in selection.picks() {
                check_pick(index, matchers, site, pick, &moved, findings);
                check_witness_body(ctx, document, site.holder.record(), pick, findings)?;
            }
        }
    }
    for publication in &index.records.ir.design.publications {
        let site = Site {
            holder: Holder::Record {
                record: publication.id.as_str(),
                scope: Some(&publication.scope),
            },
            anchor: &Anchor::Current,
            owner: Some(&publication.scope),
        };
        check_pick(index, matchers, site, &publication.target, &moved, findings);
        check_witness_body(
            ctx,
            document,
            publication.id.as_str(),
            &publication.target,
            findings,
        )?;
    }
    for evaluation in &index.records.ir.design.evaluations {
        for path in evaluation
            .content
            .iter()
            .flat_map(|content| content.delta.instance_bodies.keys())
        {
            let at = evaluation.id.as_str();
            check_unconfigured(path, at, "an instance-body path", findings);
            check_path(
                &index.records,
                Some(evaluation.scope.as_str()),
                path,
                &moved,
                at,
                findings,
            );
        }
    }
    Ok(())
}

/// The anchors a holder admits (§5.8 Anchors).
fn check_anchor<'a>(
    index: &DesignIndex<'a>,
    site: Site<'a>,
    selection: &SelectionView<'a>,
    findings: &mut Vec<Finding>,
) {
    let at = site.holder.record();
    let feature = site.holder.feature();
    match selection.at {
        Anchor::Input => {
            let ordered =
                feature.is_some_and(|feature| index.in_ordered(MemberKey::Feature(feature)));
            if !ordered {
                super::error(findings, at, crate::rule!("design.selection-anchored-input-not-definition-sketch", 
                        "a selection anchored Input is not in the definition, a sketch, or a parameter of a feature of an ordered container"
                    ),
                );
            }
        }
        Anchor::Pinned(position) => {
            if position_resolves(index, position) {
                check_resulting_position(&index.records, PositionKey::of(position), at, findings);
            } else {
                super::error(
                    findings,
                    at,
                    crate::rule!(
                        "design.selection-pinned-position-does-not-resolve",
                        "a selection is pinned to a position that does not resolve"
                    ),
                );
            }
        }
        Anchor::Current | Anchor::Native(_) => {}
    }
    if let Some(feature) = feature {
        if let Some((
            container @ crate::design::container::Container::Unordered { members, .. },
            _,
        )) = index.place(MemberKey::Feature(feature))
        {
            let admitted = match selection.at {
                Anchor::Input | Anchor::Current | Anchor::Native(_) => true,
                Anchor::Pinned(Position::After(Member::Feature(other))) => {
                    other.as_str() != feature && members.contains(&Member::Feature(other.clone()))
                }
                Anchor::Pinned(_) => false,
            };
            if !admitted {
                super::error(findings, at, crate::rule!("design.selection-member-unordered-container-pinned-state", 
                        "a selection of a member of unordered container {} is pinned to a state other than another member's result",
                        container.id().as_str()
                    ),
                );
            }
        }
    }
    if let Anchor::Native(_) = selection.at {
        let scoped = selection.picks().into_iter().any(|pick| {
            resolving_definition(index, site.owner, pick)
                .is_some_and(|definition| index.records.scopes.contains_key(definition))
        });
        if scoped {
            super::error(findings, at, crate::rule!("design.selection-anchored-native-picks-definition-scope", "a selection anchored Native picks in a definition that has a scope in this document"),
            );
        }
    }
}

/// Whether a position names existing members, and `Start` and
/// `After(Container(c))` an ordered container.
fn position_resolves(index: &DesignIndex<'_>, position: &Position) -> bool {
    match position {
        Position::Start(container) => matches!(
            index.records.containers.get(container.as_str()),
            Some(crate::design::container::Container::Ordered { .. })
        ),
        Position::After(Member::Feature(feature)) => {
            index.place(MemberKey::Feature(feature.as_str())).is_some()
        }
        Position::After(Member::Container(container)) => matches!(
            index.records.containers.get(container.as_str()),
            Some(crate::design::container::Container::Ordered { .. })
        ),
    }
}

/// The definition a pick resolves in: its context's, else the owner's.
/// `None` where a path runs through an occurrence the document does not
/// hold.
pub(super) fn resolving_definition<'a>(
    index: &DesignIndex<'a>,
    owner: Option<&'a ProductDefinitionId>,
    pick: &'a Picked,
) -> Option<&'a str> {
    let last = |path: &'a OccurrencePath| -> Option<&'a str> {
        match &path.0.last()?.occurrence {
            StepRef::Occurrence(occurrence) => index.records.prototype(occurrence.as_str()),
            StepRef::Native(_) => None,
        }
    };
    match &pick.context {
        None => owner.map(ProductDefinitionId::as_str),
        Some(PickContext::Occurrence(path)) => last(path),
        Some(PickContext::Definition(DefinitionRef { definition, .. })) => {
            Some(definition.as_str())
        }
        Some(PickContext::InContext {
            assembly, target, ..
        }) => match target {
            Some(target) if !target.0.is_empty() => last(target),
            _ => Some(assembly.definition.as_str()),
        },
    }
}

/// The rules on rule terms (§5.8 Rules, One spelling).
fn check_rules(selection: &SelectionView<'_>, site: Site<'_>, findings: &mut Vec<Finding>) {
    let PicksView::Rule {
        terms, excluded, ..
    } = &selection.picks
    else {
        return;
    };
    let at = site.holder.record();
    let mut record = |message: Ruled<'_>| super::error(findings, at, message);
    for term in terms.iter().chain(excluded) {
        let seeded = !term.seeds.is_empty();
        match term.rule {
            SelectionRule::All { .. } if seeded => {
                record(crate::rule!(
                    "design.all-rule-term-seeds",
                    "an All rule term has seeds"
                ));
            }
            SelectionRule::Affected if seeded => {
                record(crate::rule!(
                    "design.affected-rule-term-seeds",
                    "an Affected rule term has seeds"
                ));
            }
            SelectionRule::All { .. } | SelectionRule::Affected => {}
            _ if !seeded => record(crate::rule!(
                "design.rule-term-other-all-affected-no",
                "a rule term other than All or Affected has no seeds"
            )),
            _ => {}
        }
        if matches!(term.rule, SelectionRule::Affected) {
            if term.propagation.is_some() {
                record(crate::rule!(
                    "design.affected-rule-term-states-propagation",
                    "an Affected rule term states a propagation"
                ));
            }
            if !selection.participants {
                record(crate::rule!(
                    "design.affected-rule-term-outside-assembly-context",
                    "an Affected rule term is outside an assembly context's participants"
                ));
            }
        }
        if let SelectionRule::Region { boundary } = term.rule {
            let curves = boundary.as_slice().iter().any(picks_curve);
            let containing = term
                .seeds
                .iter()
                .any(|seed| matches!(seed.domain, Some(PickDomain::Containing(_))));
            if curves && !containing {
                record(crate::rule!("design.region-term-boundary-picks-curves-no", "a Region term whose boundary picks curves has no seed with a Containing domain"),
                );
            }
        }
    }
    let explicit = |term: &&TermView<'_>| matches!(term.rule, SelectionRule::Explicit);
    if let [only] = terms.as_slice() {
        if explicit(&only) && only.propagation.is_none() && excluded.is_empty() {
            record(crate::rule!("design.rule-selection-only-term-explicit-no", "a rule selection whose only term is Explicit, with no propagation and no exclusions, is Named"),
            );
        }
    }
    let explicit_terms: Vec<&TermView<'_>> = terms.iter().filter(explicit).collect();
    for (position, term) in explicit_terms.iter().enumerate() {
        if explicit_terms[..position]
            .iter()
            .any(|earlier| earlier.propagation == term.propagation)
        {
            record(crate::rule!(
                "design.two-explicit-terms-one-selection-equal",
                "two Explicit terms of one selection have equal propagation"
            ));
        }
    }
}

/// Whether a boundary pick picks a curve rather than a face.
fn picks_curve(pick: &Picked) -> bool {
    match &pick.target {
        PickTarget::SketchEntity(_) | PickTarget::SpatialSketchEntity(_) => true,
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.contains(&NameKind::Curve) || kinds.contains(&NameKind::Edge)
        }
        _ => false,
    }
}

/// A pick's witness body is closed over its own references, and the rules of
/// one topology hold on it (part 10, Coincidence).
fn check_witness_body(
    ctx: &DecodeContext<'_>,
    document: NativeView<'_>,
    at: &str,
    pick: &Picked,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let Some(body) = pick
        .witness
        .as_ref()
        .and_then(|witness| witness.body.as_deref())
    else {
        return Ok(());
    };
    let nested =
        crate::validate::nested_topology_findings(ctx, document, body.clone(), true, true)?;
    for finding in nested {
        findings.push(crate::validate::nested_finding(
            finding,
            Check::Design,
            at,
            "witness body",
        ));
    }
    Ok(())
}

/// The rules on one pick: its target, its context, and its caches.
fn check_pick<'a>(
    index: &DesignIndex<'a>,
    matchers: &Matchers<'a>,
    site: Site<'a>,
    pick: &'a Picked,
    moved: &BTreeSet<&str>,
    findings: &mut Vec<Finding>,
) {
    let at = site.holder.record();
    if !target_exists(index, &pick.target) {
        super::error(
            findings,
            at,
            crate::rule!(
                "design.pick-names-does-not-exist",
                "a pick names a {} that does not exist",
                target_kind(&pick.target)
            ),
        );
    } else if pick.context.is_none() {
        check_target_order(index, site, pick, findings);
    }
    if let PickTarget::Native { .. } = pick.target {
        if pick.native.is_none() && pick.witness.is_none() {
            super::error(
                findings,
                at,
                crate::rule!(
                    "design.native-pick-carries-neither-native-name",
                    "a Native pick carries neither a native name nor a witness"
                ),
            );
        }
    }
    check_datum_part(index, pick, at, findings);
    check_principal(index, site, pick, findings);
    match &pick.context {
        Some(PickContext::Occurrence(path)) => check_path(
            &index.records,
            site.owner.map(ProductDefinitionId::as_str),
            path,
            moved,
            at,
            findings,
        ),
        Some(PickContext::Definition(definition)) => {
            check_definition(index, definition, at, findings);
        }
        Some(PickContext::InContext {
            assembly,
            own,
            target,
        }) => {
            check_definition(index, assembly, at, findings);
            for path in own.iter().chain(target) {
                check_path(
                    &index.records,
                    Some(assembly.definition.as_str()),
                    path,
                    moved,
                    at,
                    findings,
                );
            }
        }
        None => {}
    }
    if let PickTarget::Component = pick.target {
        let named = match &pick.context {
            Some(PickContext::Occurrence(path)) => !path.0.is_empty(),
            Some(PickContext::InContext {
                target: Some(path), ..
            }) => !path.0.is_empty(),
            _ => false,
        };
        if !named {
            super::error(
                findings,
                at,
                crate::rule!(
                    "design.component-pick-context-names-no-occurrence",
                    "a Component pick's context names no occurrence"
                ),
            );
        }
    }
    if pick.context.is_none() {
        check_anchored(index, matchers, site, pick, findings);
    }
}

/// A pick of a process record with no context, which resolves in its
/// setup's scope (§5.8, part 7).
pub(in crate::validate) struct ProcessPick<'a> {
    pub(in crate::validate) record: &'a str,
    pub(in crate::validate) scope: &'a ProductDefinitionId,
    pub(in crate::validate) anchor: &'a Anchor,
    pub(in crate::validate) pick: &'a Picked,
}

/// Each process pick resolves in its anchored state by the matching rule
/// of §5.7, as a design pick does: a `Current` pick in the current geometry
/// of its setup's scope (§5.8).
pub(in crate::validate) fn check_process_picks(
    ir: &CadIr,
    picks: &[ProcessPick<'_>],
    findings: &mut Vec<Finding>,
) {
    if picks.is_empty() {
        return;
    }
    let index = DesignIndex::new(ir);
    let states = States::new(ir);
    let matchers = Matchers::new(&states);
    for pick in picks {
        let site = Site {
            holder: Holder::Record {
                record: pick.record,
                scope: Some(pick.scope),
            },
            anchor: pick.anchor,
            owner: Some(pick.scope),
        };
        check_anchored(&index, &matchers, site, pick.pick, findings);
    }
}

/// A pick with no context resolves in each stored state its anchor names.
fn check_anchored<'a>(
    index: &DesignIndex<'a>,
    matchers: &Matchers<'a>,
    site: Site<'a>,
    pick: &'a Picked,
    findings: &mut Vec<Finding>,
) {
    let states = matchers.states();
    let at = site.holder.record();
    let stored = anchored_states(index, states, site);
    if stored.is_empty() && pick.resolved.is_some() && names_unstored_state(index, site) {
        super::error(
            findings,
            at,
            crate::rule!(
                "design.resolved-without-stored-state",
                "a pick states `resolved`, but the document stores no state at its anchor"
            ),
        );
    }
    for state in stored {
        check_in_state(states, matchers, &state, pick, at, findings);
    }
}

/// Whether the anchor of a pick with no context names a state of its scope
/// that the document does not store (§5.8): a position with no current
/// state there and no evaluation with content of it, or `Current` in a scope
/// with no current state and no unproduced body. `false` where the anchor
/// is `Native` or names no position the document states.
fn names_unstored_state(index: &DesignIndex<'_>, site: Site<'_>) -> bool {
    let Some(scope) = site.owner.map(ProductDefinitionId::as_str) else {
        return false;
    };
    let position = match site.anchor {
        Anchor::Native(_) => return false,
        Anchor::Current => {
            return index
                .records
                .scopes
                .get(scope)
                .is_some_and(|scope| scope.current.is_empty() && scope.unproduced.is_empty());
        }
        Anchor::Pinned(position) => Some(PositionKey::of(position)),
        Anchor::Input => site
            .holder
            .feature()
            .and_then(|feature| index.entering(feature)),
    };
    position.is_some_and(|position| index.rollback_position(position).is_some())
}

/// A stored state a pick resolves in.
pub(super) enum Stored<'a> {
    /// The extents of current states.
    Extents(Vec<&'a Extent<'a>>),
    /// A stored evaluation, by id.
    Evaluation(&'a str),
}

impl<'a> Stored<'a> {
    /// The configuration the state is resolved in: the scope's active
    /// configuration for current states, the evaluation's own for a stored
    /// evaluation.
    pub(super) fn configuration(&self, records: &DesignRecords<'a>) -> Option<&'a str> {
        let configuration = match self {
            Self::Extents(extents) => extents.first().and_then(|extent| {
                records
                    .scopes
                    .get(extent.scope)?
                    .active_configuration
                    .as_ref()
            }),
            Self::Evaluation(evaluation) => records
                .ir
                .design
                .evaluations
                .iter()
                .find(|record| record.id.as_str() == *evaluation)?
                .configuration
                .as_ref(),
        };
        configuration.map(ConfigurationId::as_str)
    }
}

/// The stored states a pick without a context resolves in (§5.8): the
/// current geometry of its scope for `Current`; for a position, the current
/// state there, else each stored evaluation of it, the end of an ordered
/// container being the position at its rollback. None where the anchor
/// names no stored state, or is `Native`.
pub(super) fn anchored_states<'a>(
    index: &DesignIndex<'a>,
    states: &'a States<'a>,
    site: Site<'a>,
) -> Vec<Stored<'a>> {
    let Some(scope) = site.owner.map(ProductDefinitionId::as_str) else {
        return Vec::new();
    };
    let position = match site.anchor {
        Anchor::Current => {
            let extents: Vec<&Extent<'a>> = states
                .extents
                .iter()
                .filter(|extent| extent.scope == scope)
                .collect();
            return if extents.is_empty() {
                Vec::new()
            } else {
                vec![Stored::Extents(extents)]
            };
        }
        Anchor::Native(_) => return Vec::new(),
        Anchor::Pinned(position) => PositionKey::of(position),
        Anchor::Input => match site
            .holder
            .feature()
            .and_then(|feature| index.entering(feature))
        {
            Some(position) => position,
            None => return Vec::new(),
        },
    };
    let Some(position) = index.rollback_position(position) else {
        return Vec::new();
    };
    if let Some(extent) = states.extent(scope, position) {
        return vec![Stored::Extents(vec![extent])];
    }
    index
        .records
        .ir
        .design
        .evaluations
        .iter()
        .filter(|evaluation| {
            evaluation.scope.as_str() == scope
                && evaluation.content.is_some()
                && matches!(&evaluation.of, Evaluated::State(of) if PositionKey::of(of) == position)
        })
        .map(|evaluation| Stored::Evaluation(evaluation.id.as_str()))
        .collect()
}

/// An `Entity` pick matches an entity of its stored anchored state by the
/// matching rule of §5.7, and its `resolved` entries are records of it.
fn check_in_state<'a>(
    states: &States<'a>,
    matchers: &Matchers<'a>,
    state: &Stored<'a>,
    pick: &Picked,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let holds = |entity: &EntityRef| match state {
        Stored::Extents(extents) => {
            states.geometry_holds(entity)
                && extents
                    .iter()
                    .any(|extent| extent.records.contains(entity.id()))
        }
        Stored::Evaluation(evaluation) => states.holds(evaluation, entity) == Some(true),
    };
    for entity in pick.resolved.iter().flatten() {
        if !holds(entity) {
            super::error(
                findings,
                at,
                crate::rule!(
                    "design.pick-resolved-entity-not-record-anchored",
                    "a pick's resolved entity {} is not a record of its anchored state",
                    entity.id()
                ),
            );
        }
    }
    let PickTarget::Entity(name) = &pick.target else {
        return;
    };
    let Some(matcher) = matchers.get(state) else {
        return;
    };
    if !matcher.matches_any(name) {
        record_finding(
            findings,
            Check::Design,
            Severity::Warning,
            Some(at),
            crate::rule!("design.entity-pick-name-matches-no-entity", "an Entity pick's name matches no entity of its anchored state, a lost operand (D51)"),
        );
    }
}

fn target_kind(target: &PickTarget) -> &'static str {
    match target {
        PickTarget::Entity(_) => "entity",
        PickTarget::Native { .. } => "native owner",
        PickTarget::Sketch(_) => "sketch",
        PickTarget::SketchEntity(_) => "sketch entity",
        PickTarget::SpatialSketchEntity(_) => "spatial sketch entity",
        PickTarget::SketchRegion { .. } => "region's sketch",
        PickTarget::SketchFrame { .. } => "frame's sketch",
        PickTarget::Datum { .. } => "datum feature",
        PickTarget::Principal { .. } => "principal container",
        PickTarget::Feature(_) => "feature",
        PickTarget::FeatureResult(_) => "result's feature",
        PickTarget::TemplateInput { .. } => "template instance",
        PickTarget::Component => "component",
        PickTarget::Publication(_) => "publication",
    }
}

/// Whether the record a target names exists.
fn target_exists(index: &DesignIndex<'_>, target: &PickTarget) -> bool {
    use crate::design::selection::NativeOwner;
    match target {
        PickTarget::Sketch(sketch)
        | PickTarget::SketchRegion { sketch, .. }
        | PickTarget::SketchFrame { sketch, .. } => index.sketches.contains_key(sketch.as_str()),
        PickTarget::SketchEntity(entity) => index.sketch_entities.contains_key(entity.as_str()),
        PickTarget::SpatialSketchEntity(entity) => {
            index.spatial_sketch_entities.contains_key(entity.as_str())
        }
        PickTarget::Datum { feature, .. }
        | PickTarget::Feature(feature)
        | PickTarget::FeatureResult(feature)
        | PickTarget::TemplateInput {
            instance: feature, ..
        } => index.records.features.contains_key(feature.as_str()),
        PickTarget::Publication(publication) => index.publications.contains(publication.as_str()),
        PickTarget::Native { within } => match within {
            None => true,
            Some(NativeOwner::Sketch(sketch)) => index.sketches.contains_key(sketch.as_str()),
            Some(NativeOwner::SpatialSketch(sketch)) => {
                index.spatial_sketches.contains_key(sketch.as_str())
            }
            Some(NativeOwner::Feature(feature)) => {
                index.records.features.contains_key(feature.as_str())
            }
        },
        PickTarget::Principal { container, .. } => container
            .as_ref()
            .is_none_or(|container| index.records.containers.contains_key(container.as_str())),
        PickTarget::Entity(_) | PickTarget::Component => true,
    }
}

/// A target's feature depend-orders at or before the anchor (§5.3); a
/// back-edge inside a dependency cycle is that cycle's Warning.
fn check_target_order<'a>(
    index: &DesignIndex<'a>,
    site: Site<'a>,
    pick: &'a Picked,
    findings: &mut Vec<Finding>,
) {
    let Some(target) = index.target_feature(&pick.target) else {
        return;
    };
    let holder = site.holder.feature();
    let features = match site.anchor {
        Anchor::Input => match holder {
            Some(feature) => index.state_features(StateAt::Entering(feature)),
            None => None,
        },
        Anchor::Pinned(position) => index.state_features(StateAt::Position(position)),
        Anchor::Current => site
            .owner
            .and_then(|owner| index.current_features(owner.as_str())),
        Anchor::Native(_) => None,
    };
    let Some(features) = features else {
        return;
    };
    if features.contains(target) {
        return;
    }
    let at = site.holder.record();
    let cycle = holder.is_some_and(|feature| index.depends_on(target, feature));
    let severity = if cycle {
        Severity::Warning
    } else {
        Severity::Error
    };
    record_finding(
        findings,
        Check::Design,
        severity,
        Some(at),
        crate::rule!(
            "design.pick-names-feature-does-not-depend",
            "a pick names feature {target}, which does not depend-order before its anchor{}",
            if cycle {
                " (a back-edge of a dependency cycle)"
            } else {
                ""
            }
        ),
    );
}

/// A datum part names a part the datum has.
fn check_datum_part(index: &DesignIndex<'_>, pick: &Picked, at: &str, findings: &mut Vec<Finding>) {
    let PickTarget::Datum {
        feature,
        part: Some(part),
    } = &pick.target
    else {
        return;
    };
    let admitted = match index.operation(feature.as_str()) {
        None | Some(FeatureOperation::Unresolved { .. } | FeatureOperation::Native { .. }) => true,
        Some(FeatureOperation::Datum(datum)) => match (part, &datum.element) {
            (DatumPart::Principal(_), DatumElement::Frame(_)) => true,
            (DatumPart::Point(point), DatumElement::Points(points)) => {
                usize::try_from(*point).is_ok_and(|point| point < points.len())
            }
            _ => false,
        },
        Some(_) => false,
    };
    if !admitted {
        super::error(
            findings,
            at,
            crate::rule!(
                "design.pick-names-part-datum-feature-does",
                "a pick names a part that datum feature {} does not have",
                feature.as_str()
            ),
        );
    }
}

/// `Principal.container` names an ordered container of the scope that spans
/// the definition the pick resolves in (§5.1, §5.2).
fn check_principal<'a>(
    index: &DesignIndex<'a>,
    site: Site<'a>,
    pick: &'a Picked,
    findings: &mut Vec<Finding>,
) {
    let PickTarget::Principal {
        container: Some(container),
        ..
    } = &pick.target
    else {
        return;
    };
    let Some(record) = index.records.containers.get(container.as_str()) else {
        return;
    };
    let ordered = matches!(record, crate::design::container::Container::Ordered { .. });
    let definition = resolving_definition(index, site.owner, pick);
    let scope = index.scope_of(MemberKey::Container(container.as_str()));
    let elsewhere = matches!((definition, scope), (Some(definition), Some(scope))
        if definition != scope.as_str()
            && !index.records.spanned_definitions(scope.as_str()).contains(definition));
    if !ordered || elsewhere {
        super::error(findings, site.holder.record(), crate::rule!("design.principal-pick-names-container-not-ordered", "a Principal pick names container {}, which is not an ordered container of the scope that spans its definition", container.as_str()),
        );
    }
}

fn check_definition(
    index: &DesignIndex<'_>,
    definition: &DefinitionRef,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    if let Some(configuration) = &definition.configuration {
        check_configuration_ref(
            &index.records,
            configuration,
            Some(definition.definition.as_str()),
            at,
            findings,
        );
    }
    if !index
        .records
        .definitions
        .contains_key(definition.definition.as_str())
    {
        super::error(
            findings,
            at,
            crate::rule!(
                "design.pick-context-names-definition-does-not",
                "a pick context names definition {}, which does not exist",
                definition.definition.as_str()
            ),
        );
    }
}

/// The occurrences whose owner some state restates, whose membership the
/// current listing does not decide.
pub(in crate::validate) fn moved_occurrences(ir: &CadIr) -> BTreeSet<&str> {
    ir.design
        .evaluations
        .iter()
        .flat_map(|evaluation| evaluation.content.iter())
        .flat_map(|content| content.delta.members.keys())
        .map(crate::ids::OccurrenceId::as_str)
        .collect()
}

/// A path that states no step configuration: a nested override's, an
/// instance-body entry's, a representation member's, or an instance
/// target's (§5.8).
pub(in crate::validate) fn check_unconfigured(
    path: &OccurrencePath,
    at: &str,
    what: &str,
    findings: &mut Vec<Finding>,
) {
    if path.0.iter().any(|step| step.configuration.is_some()) {
        super::error(
            findings,
            at,
            crate::rule!(
                "design.path-states-step-configuration",
                "{what} states a step configuration"
            ),
        );
    }
}

/// An occurrence path resolves step by step from its root definition: each
/// `Occurrence` step names an occurrence placed in the previous step's
/// prototype, or in the root for the first step; every step after a
/// `Native` step is `Native`. A step's `configuration` follows the
/// `ConfigurationRef` rule for that step's prototype.
pub(in crate::validate) fn check_path(
    records: &DesignRecords<'_>,
    root: Option<&str>,
    path: &OccurrencePath,
    moved: &BTreeSet<&str>,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let mut native = false;
    let mut previous = root;
    for step in &path.0 {
        let occurrence = match &step.occurrence {
            StepRef::Native(_) => {
                native = true;
                previous = None;
                continue;
            }
            StepRef::Occurrence(_) if native => {
                super::error(
                    findings,
                    at,
                    crate::rule!(
                        "design.occurrence-path-names-occurrence-after-native",
                        "an occurrence path names an occurrence after a Native step"
                    ),
                );
                return;
            }
            StepRef::Occurrence(occurrence) => occurrence,
        };
        let Some(placed) = records.occurrences.get(occurrence.as_str()) else {
            super::error(
                findings,
                at,
                crate::rule!(
                    "design.occurrence-path-names-occurrence-does-not",
                    "an occurrence path names occurrence {}, which does not exist",
                    occurrence.as_str()
                ),
            );
            return;
        };
        if let (Some(expected), false) = (previous, moved.contains(occurrence.as_str())) {
            let placed_in = records.occurrence_owner(occurrence.as_str());
            if placed_in.is_some_and(|placed_in| placed_in != expected) {
                super::error(
                    findings,
                    at,
                    crate::rule!("design.occurrence-path-step-not-occurrence-previous", 
                    "occurrence path step {} is not an occurrence of the previous step's prototype",
                    occurrence.as_str()
                ),
                );
                return;
            }
        }
        if let Some(configuration) = &step.configuration {
            check_configuration_ref(
                records,
                configuration,
                Some(placed.prototype.as_str()),
                at,
                findings,
            );
        }
        previous = Some(placed.prototype.as_str());
    }
}

/// Every `TemplateInput` pick names an input of an instance that lists the
/// picking feature as a member: the feature whose definition holds the pick,
/// or that owns the sketch, sketch record, or parameter that holds it.
pub(super) fn check_template_inputs(
    index: &DesignRecords<'_>,
    held: &[Held<'_>],
    findings: &mut Vec<Finding>,
) {
    for record in held {
        let Some(picking) = record.holder.feature() else {
            continue;
        };
        for selection in &record.selections {
            for pick in selection.picks() {
                let PickTarget::TemplateInput { instance, input } = &pick.target else {
                    continue;
                };
                let valid = match index
                    .features
                    .get(instance.as_str())
                    .map(|feature| &feature.definition.operation)
                {
                    Some(FeatureOperation::TemplateInstance(template)) => {
                        template
                            .members
                            .iter()
                            .any(|member| member.as_str() == picking)
                            && usize::try_from(*input)
                                .is_ok_and(|input| input < template.inputs.len())
                    }
                    _ => false,
                };
                if !valid {
                    record_finding(
                        findings,
                        Check::Design,
                        Severity::Error,
                        Some(record.holder.record()),
                        crate::rule!(
                            "design.templateinput-picks-input-no-input-instance",
                            "TemplateInput picks input {input} of {}, which is no input of an instance that lists this feature",
                            instance.as_str()
                        ),
                    );
                }
            }
        }
    }
}
