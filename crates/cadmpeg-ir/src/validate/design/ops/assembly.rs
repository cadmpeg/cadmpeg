// SPDX-License-Identifier: Apache-2.0
//! Assembly operations (§6.7).

use crate::report::check::Ruled;
use std::collections::{BTreeMap, BTreeSet};

use super::picks::{self, Class};
use super::{body, boolean_field, error, BooleanField, Site};
use crate::design::construction::FrameAxis;
use crate::design::container::Container;
use crate::design::feature::{Driver, Feature, FeatureOperation, OperationFamily};
use crate::design::ops::assembly::{
    AssemblyContext, ComponentPattern, ContextEffect, MirrorComponents, Place, PlaceSet,
    PlacementBase, RigidGroup,
};
use crate::design::ops::body::{Kept, ScaleFactors, SplitMode};
use crate::design::ops::dressup::StageHole;
use crate::design::ops::shared::{
    AxisLimits, Bounds, ChainLinks, Joint, JointConnector, JointKind, JointMotion, PathAxisControl,
    PathRollControl,
};
use crate::design::ops::sheet_metal::SheetMetalKind;
use crate::design::options::Boolean;
use crate::design::selection::{
    Keyed, Members, OccurrencePath, OrderedSelection, PickContext, PickTarget, Picked, Picks,
    SelectionRule, SetSelection, StepRef,
};
use crate::ids::OccurrenceId;
use crate::report::check::Finding;
use crate::scalar::{Angle, Length};

use super::super::constructions::input_selections;
use super::super::dimensions::Bound;
use super::super::records::{DesignRecords, MemberKey, Namer};

/// The definition whose `occurrences` lists an occurrence (§8.1).
fn owner<'a>(index: &DesignRecords<'a>, occurrence: &str) -> Option<&'a str> {
    index
        .ir
        .model
        .product_definitions
        .iter()
        .find(|definition| {
            definition
                .occurrences
                .iter()
                .any(|o| o.as_str() == occurrence)
        })
        .map(|definition| definition.id.as_str())
}

/// An occurrence a feature names exists and is owned by a definition the
/// feature's scope spans.
fn check_owned(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    field: &str,
    occurrence: &str,
    findings: &mut Vec<Finding>,
) {
    if !index.occurrences.contains_key(occurrence) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.names-no-occurrence-model",
                "{field} names {occurrence}, which is no occurrence of the model"
            ),
        );
        return;
    }
    let Some(owner) = owner(index, occurrence) else {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.names-no-definition-owns",
                "{field} names {occurrence}, which no definition owns"
            ),
        );
        return;
    };
    let Some(scope) = site.scope else {
        return;
    };
    if !index.spanned_definitions(scope).contains(owner) {
        error(
            findings,
            site.entity,
            crate::rule!("design.names-owned-feature-scope-does-not", "{field} names {occurrence}, owned by {owner}, which the feature's scope does not span"),
        );
    }
}

pub(super) fn check_place(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    place: &Place,
    findings: &mut Vec<Finding>,
) {
    let occurrence = place.occurrence.as_str();
    check_owned(index, site, "Place.occurrence", occurrence, findings);
    check_own_occurrence(index, site, place, findings);
    if place.base.is_some() != place.placement.is_some() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.place-base-present-where-placement-not",
                "Place.base is present where placement is not, or absent where it is"
            ),
        );
    }
    match &place.scale {
        Some(_) if place.placement.is_none() => error(
            findings,
            site.entity,
            crate::rule!(
                "design.place-scale-present-where-placement-not",
                "Place.scale is present where placement is not"
            ),
        ),
        Some(ScaleFactors::PerAxis { frame: Some(_), .. }) => error(
            findings,
            site.entity,
            crate::rule!(
                "design.peraxis-place-scale-states-frame",
                "a PerAxis Place.scale states a frame"
            ),
        ),
        _ => {}
    }
    for constraint in &place.constraints {
        check_joint(index, site, constraint, findings);
        let picks_into = constraint
            .connectors
            .iter()
            .any(|connector| super::wire_strings(connector).contains(occurrence));
        if !picks_into {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.place-constraint-no-connector-picks",
                    "a Place constraint has no connector that picks into {occurrence}"
                ),
            );
        }
    }
}

pub(super) fn check_place_set(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    set: &PlaceSet,
    findings: &mut Vec<Finding>,
) {
    let distinct: BTreeSet<&str> = set.occurrences.iter().map(OccurrenceId::as_str).collect();
    let least = if set.native_options.is_empty() { 2 } else { 1 };
    if set.occurrences.len() < least || distinct.len() != set.occurrences.len() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.placeset-lists-occurrence-twice-fewer",
                "a PlaceSet lists an occurrence twice, or fewer than {least}"
            ),
        );
    }
    for occurrence in &set.occurrences {
        check_owned(
            index,
            site,
            "PlaceSet.occurrences",
            occurrence.as_str(),
            findings,
        );
    }
}

/// The degrees of freedom a motion leaves: (rotation, axis) and
/// (translation, axis). `None` where the motion's meaning is not
/// established.
fn free_axes(motion: &JointMotion) -> Option<BTreeSet<(bool, FrameAxis)>> {
    const AXES: [FrameAxis; 3] = [FrameAxis::X, FrameAxis::Y, FrameAxis::Z];
    let free = match motion {
        JointMotion::Rigid => BTreeSet::new(),
        JointMotion::Revolute { axis } => BTreeSet::from([(true, *axis)]),
        JointMotion::Slider { axis } => BTreeSet::from([(false, *axis)]),
        JointMotion::Cylindrical { axis } => BTreeSet::from([(true, *axis), (false, *axis)]),
        JointMotion::Planar { axis } => AXES
            .iter()
            .filter(|other| *other != axis)
            .map(|other| (false, *other))
            .chain([(true, *axis)])
            .collect(),
        JointMotion::Ball => AXES.iter().map(|axis| (true, *axis)).collect(),
        JointMotion::PinSlot { axis, slide } => BTreeSet::from([(true, *axis), (false, *slide)]),
        JointMotion::Native(_) => return None,
    };
    Some(free)
}

fn check_bounds<Q: Bound + Clone>(
    site: Site<'_, '_>,
    bounds: &Bounds<Q>,
    value: impl Fn(Q) -> f64,
    findings: &mut Vec<Finding>,
) {
    if bounds.min.is_none() && bounds.max.is_none() {
        error(
            findings,
            site.entity,
            crate::rule!("design.bounds-holds-no-bound", "Bounds holds no bound"),
        );
    }
    if let (Some(min), Some(max)) = (&bounds.min, &bounds.max) {
        site.bound(findings, |values| {
            (value(values.get(min)?) > value(values.get(max)?))
                .then(|| "Bounds has min above max".to_owned())
        });
    }
}

/// A joint's connectors fit its kind; its motion's limits name free axes.
pub(super) fn check_joint(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    joint: &Joint,
    findings: &mut Vec<Finding>,
) {
    check_connectors(site, &joint.connectors, &joint.kind, findings);
    check_coupling_connectors(site, joint, findings);
    match &joint.kind {
        JointKind::Motion { motion, limits, .. } => {
            if let JointMotion::PinSlot { axis, slide } = motion {
                if axis == slide {
                    error(
                        findings,
                        site.entity,
                        crate::rule!("design.pinslot-slide-axis", "a PinSlot's slide is its axis"),
                    );
                }
            }
            let free = free_axes(motion);
            let mut named = BTreeSet::new();
            for limit in limits {
                let (rotation, axis) = match limit {
                    AxisLimits::Rotation { axis, limits, .. } => {
                        if let Some(bounds) = limits {
                            check_bounds(site, bounds, Angle::get, findings);
                        }
                        (true, *axis)
                    }
                    AxisLimits::Translation { axis, limits, .. } => {
                        if let Some(bounds) = limits {
                            check_bounds(site, bounds, Length::get, findings);
                        }
                        (false, *axis)
                    }
                };
                if free
                    .as_ref()
                    .is_some_and(|free| !free.contains(&(rotation, axis)))
                {
                    error(
                        findings,
                        site.entity,
                        crate::rule!(
                            "design.axislimits-names-degree-freedom-motion-does",
                            "AxisLimits names a degree of freedom the motion does not leave"
                        ),
                    );
                }
                if !named.insert((rotation, axis)) {
                    error(
                        findings,
                        site.entity,
                        crate::rule!(
                            "design.axislimits-names-one-axis-twice",
                            "AxisLimits names one axis twice"
                        ),
                    );
                }
            }
        }
        JointKind::Distance {
            limits: Some(bounds),
            ..
        } => check_bounds(site, bounds, Length::get, findings),
        JointKind::Angle {
            limits: Some(bounds),
            ..
        } => check_bounds(site, bounds, Angle::get, findings),
        JointKind::Path {
            pitch_yaw: Some(PathAxisControl::FollowPath { axis: pitch, .. }),
            roll: Some(PathRollControl::UpVector { axis: roll, .. }),
            ..
        } if pitch == roll => error(
            findings,
            site.entity,
            crate::rule!(
                "design.path-joint-pitch-yaw-roll-name",
                "a Path joint's pitch_yaw and roll name one axis"
            ),
        ),
        JointKind::Coupling {
            joints: Some(joints),
            ..
        } => {
            let targets: Option<Vec<&str>> = picks::named(joints).map(|picks| {
                picks
                    .iter()
                    .filter_map(|pick| match &pick.target {
                        PickTarget::Feature(feature) => Some(feature.as_str()),
                        _ => None,
                    })
                    .collect()
            });
            let fits = picks::count(joints).is_none_or(|count| count == 2)
                && targets
                    .as_ref()
                    .is_none_or(|targets| targets.len() == 2 && !targets.contains(&site.feature));
            if !fits {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.coupling-joints-picks-other-two-other",
                        "Coupling.joints picks other than two other Joint features"
                    ),
                );
            }
            for target in targets.unwrap_or_default() {
                let stated = index.features.get(target).map(|f| &f.definition.operation);
                let joint = match stated {
                    None
                    | Some(
                        FeatureOperation::Joint(_)
                        | FeatureOperation::Unresolved {
                            family: OperationFamily::Joint,
                        }
                        | FeatureOperation::Native { .. },
                    ) => true,
                    Some(_) => false,
                };
                if !joint {
                    error(
                        findings,
                        site.entity,
                        crate::rule!(
                            "design.coupling-joints-picks-no-joint-feature",
                            "Coupling.joints picks {target}, which is no Joint feature"
                        ),
                    );
                }
            }
        }
        _ => {}
    }
}

/// The connector variants a joint kind admits, and a world connector's
/// resolved value. A connector with no inputs and a step is placed by its
/// steps and is not taken for a world connector.
pub(super) fn check_connectors(
    site: Site<'_, '_>,
    connectors: &[JointConnector; 2],
    kind: &JointKind,
    findings: &mut Vec<Finding>,
) {
    for connector in connectors {
        let unplaced = match connector {
            JointConnector::Frame(c) => {
                c.inputs.is_empty() && c.after.is_empty() && c.resolved.is_none()
            }
            JointConnector::Plane(c) => {
                c.inputs.is_empty() && c.after.is_empty() && c.resolved.is_none()
            }
            JointConnector::Axis(c) => {
                c.inputs.is_empty() && c.after.is_empty() && c.resolved.is_none()
            }
            JointConnector::Point(c) => {
                c.inputs.is_empty() && c.after.is_empty() && c.resolved.is_none()
            }
            JointConnector::Entity(_) => false,
        };
        if unplaced {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.connector-no-inputs-no-steps-no",
                    "a connector with no inputs and no steps has no resolved value"
                ),
            );
        }
    }
    let entity = |connector: &JointConnector| matches!(connector, JointConnector::Entity(_));
    let fits = match kind {
        JointKind::Motion { motion, .. } if !matches!(motion, JointMotion::Rigid) => connectors
            .iter()
            .all(|connector| matches!(connector, JointConnector::Frame(_))),
        JointKind::Concentric => connectors.iter().all(|connector| {
            matches!(
                connector,
                JointConnector::Axis(_) | JointConnector::Frame(_) | JointConnector::Entity(_)
            )
        }),
        JointKind::Tangent { .. } => connectors.iter().any(entity),
        JointKind::Slot { .. } => connectors.iter().all(entity),
        JointKind::Path { .. } => {
            entity(&connectors[0]) && matches!(connectors[1], JointConnector::Point(_))
        }
        _ => true,
    };
    if !fits {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.joint-connectors-variant-kind-does-not",
                "the joint's connectors are of a variant its kind does not take"
            ),
        );
    }
}

pub(super) fn check_rigid_group(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    group: &RigidGroup,
    findings: &mut Vec<Finding>,
) {
    if group.members.len() < 2 {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.rigidgroup-fewer-two-members",
                "a RigidGroup has fewer than two members"
            ),
        );
    }
    for path in &group.members {
        if path
            .0
            .iter()
            .any(|step| !matches!(step.occurrence, StepRef::Occurrence(_)))
        {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.rigidgroup-member-path-step-other-occurrence",
                    "a RigidGroup member path has a step other than an Occurrence"
                ),
            );
        }
        if let Some(StepRef::Occurrence(first)) = path.0.first().map(|step| &step.occurrence) {
            check_owned(index, site, "RigidGroup.members", first.as_str(), findings);
        }
    }
}

fn check_component_seeds(
    site: Site<'_, '_>,
    field: &str,
    seeds: &crate::design::selection::OrderedSelection,
    findings: &mut Vec<Finding>,
) {
    let allowed = |class| class == Class::Component;
    if let Some(class) = picks::refused(seeds, allowed) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.picks-picks-only-component-targets",
                "{field} picks {class}; it picks only Component targets"
            ),
        );
    }
}

pub(super) fn check_component_pattern(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    pattern: &ComponentPattern,
    findings: &mut Vec<Finding>,
) {
    check_component_seeds(site, "ComponentPattern.seeds", &pattern.seeds, findings);
    body::check_layout(index, site, &pattern.layout, true, None, findings);
    let seeds = picks::count(&pattern.seeds);
    if let (crate::design::ops::shared::PatternLayout::Chain(chain), Some(seeds)) =
        (&pattern.layout, seeds)
    {
        if let crate::design::ops::shared::ChainLinks::Connected(links) = &chain.links {
            if links.len() != seeds {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.chainlinks-connected-holds-links-seeds",
                        "ChainLinks::Connected holds {} links for {seeds} seeds",
                        links.len()
                    ),
                );
            }
        }
    }
    if let crate::design::ops::shared::PatternLayout::Chain(chain) = &pattern.layout {
        check_chain_seeds(site, &pattern.seeds, &chain.links, findings);
    }
    let indices: Vec<_> = pattern
        .instances
        .iter()
        .map(|instance| &instance.index)
        .collect();
    body::check_indices(index, site, &pattern.layout, &indices, findings);
    let mut seen = BTreeSet::new();
    for instance in &pattern.instances {
        if seeds.is_some_and(|seeds| instance.occurrences.len() != seeds) {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.componentinstance-lists-occurrences-seeds",
                    "a ComponentInstance lists {} occurrences for {} seeds",
                    instance.occurrences.len(),
                    seeds.unwrap_or_default()
                ),
            );
        }
        for occurrence in &instance.occurrences {
            if !seen.insert(occurrence.as_str()) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.occurrence-repeats-across-component-instances",
                        "occurrence {} repeats across component instances",
                        occurrence.as_str()
                    ),
                );
            }
            check_owned(
                index,
                site,
                "ComponentInstance.occurrences",
                occurrence.as_str(),
                findings,
            );
        }
    }
}

/// The occurrences a path steps through, where every step names one.
fn path_occurrences(path: &OccurrencePath) -> Option<Vec<&str>> {
    path.0
        .iter()
        .map(|step| match &step.occurrence {
            StepRef::Occurrence(id) => Some(id.as_str()),
            StepRef::Native(_) => None,
        })
        .collect()
}

/// The definition that owns the selections of a site's feature: its
/// `component` where stated, else its scope's definition (§5.8).
fn owner_definition<'a>(site: Site<'_, 'a>) -> Option<&'a str> {
    site.index
        .and_then(|index| index.features.get(site.feature))
        .and_then(|feature| feature.component.as_ref())
        .map(crate::ids::ProductDefinitionId::as_str)
        .or(site.scope)
}

/// A pick's occurrences from the definition it starts at: the owner's
/// definition for an `Occurrence` context, the assembly for `InContext`,
/// with `own`, the owner's own place in that assembly (§5.8). A step's
/// configuration selects a variant of its occurrence, and the occurrence
/// alone places the pick.
struct Rooted<'a> {
    root: &'a str,
    own: Option<Vec<&'a str>>,
    steps: Vec<&'a str>,
}

impl<'a> Rooted<'a> {
    /// `None` where the pick has no occurrence path from a definition, or a
    /// step is `Native`.
    fn of(pick: &'a Picked, owner: Option<&'a str>) -> Option<Self> {
        match pick.context.as_ref()? {
            PickContext::Occurrence(path) => Some(Self {
                root: owner?,
                own: None,
                steps: path_occurrences(path)?,
            }),
            PickContext::InContext {
                assembly,
                own,
                target: Some(target),
            } => Some(Self {
                root: assembly.definition.as_str(),
                own: match own {
                    Some(own) => Some(path_occurrences(own)?),
                    None => None,
                },
                steps: path_occurrences(target)?,
            }),
            _ => None,
        }
    }

    /// The steps of `self` from the root of `frame`, where `self` starts at
    /// that root or at the owner's definition that `frame` places by `own`.
    fn steps_from_root_of(&self, frame: &Self, owner: Option<&str>) -> Option<Vec<&'a str>> {
        if self.root == frame.root {
            return Some(self.steps.clone());
        }
        if self.own.is_some() || Some(self.root) != owner {
            return None;
        }
        let mut steps = frame.own.clone()?;
        steps.extend_from_slice(&self.steps);
        Some(steps)
    }

    /// Whether `self` lies under `seed`; `None` where their paths start at
    /// definitions neither places in the other.
    fn under(&self, seed: &Self, owner: Option<&str>) -> Option<bool> {
        if let Some(seed_steps) = seed.steps_from_root_of(self, owner) {
            return Some(self.steps.starts_with(&seed_steps));
        }
        self.steps_from_root_of(seed, owner)
            .map(|steps| steps.starts_with(&seed.steps))
    }
}

/// A chain's pins are two distinct axes, and its pins and native picks pick
/// the geometry of the pattern's seeds (§6.7). A pick is judged where it and
/// every seed name their occurrences by paths of occurrence steps from one
/// definition, or from an assembly and the owner's definition the
/// `InContext` pick places in it by `own`.
fn check_chain_seeds(
    site: Site<'_, '_>,
    seeds: &OrderedSelection,
    links: &ChainLinks,
    findings: &mut Vec<Finding>,
) {
    let owner = owner_definition(site);
    let seeds: Option<Vec<_>> = picks::named(seeds)
        .and_then(|seeds| seeds.iter().map(|seed| Rooted::of(seed, owner)).collect());
    let off_seeds = |selection: &OrderedSelection| {
        let Some(seeds) = seeds.as_ref().filter(|seeds| !seeds.is_empty()) else {
            return false;
        };
        picks::named(selection)
            .unwrap_or_default()
            .iter()
            .filter_map(|pick| Rooted::of(pick, owner))
            .any(|pick| {
                seeds
                    .iter()
                    .all(|seed| pick.under(seed, owner) == Some(false))
            })
    };
    match links {
        ChainLinks::Connected(links) => {
            for (at, link) in links.iter().enumerate() {
                let [first, second] = &link.pins;
                if first == second
                    || (first.resolved.is_some() && first.resolved == second.resolved)
                {
                    error(
                        findings,
                        site.entity,
                        crate::rule!(
                            "design.chain-link-pins-one-axis",
                            "the two pins of ChainLink {at} are one axis"
                        ),
                    );
                }
                let mut selections = link
                    .pins
                    .iter()
                    .flat_map(|pin| input_selections(&pin.inputs));
                if selections.any(&off_seeds) {
                    error(
                        findings,
                        site.entity,
                        crate::rule!(
                            "design.chain-link-pin-off-seeds",
                            "a pin of ChainLink {at} picks geometry of no seed of its pattern"
                        ),
                    );
                }
            }
        }
        ChainLinks::Native { picks, .. } => {
            if off_seeds(picks) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.chain-native-picks-off-seeds",
                        "ChainLinks::Native picks geometry of no seed of its pattern"
                    ),
                );
            }
        }
    }
}

pub(super) fn check_mirror(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    mirror: &MirrorComponents,
    findings: &mut Vec<Finding>,
) {
    check_component_seeds(site, "MirrorComponents.seeds", &mirror.seeds, findings);
    if picks::count(&mirror.seeds).is_some_and(|seeds| seeds != mirror.results.len()) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.mirrorcomponents-results-other-one-result-per",
                "MirrorComponents.results has other than one result per seed"
            ),
        );
    }
    for result in &mirror.results {
        check_owned(
            index,
            site,
            "MirrorComponents.results",
            result.occurrence.as_str(),
            findings,
        );
    }
}

/// Part 10 (§6.7): a definition that states `assembly` "is no `Place`,
/// `PlaceSet`, `Joint`, `RigidGroup`, `ComponentPattern`, `MirrorComponents`,
/// `Stored`, `DeleteBodies`, `TransformBodies`, `Flex`, `Graft`, `Knit`, or
/// `Mesh`, and no `Unresolved` whose `family` names one of these; it is no
/// `Split` in `Mutual` mode and no `SheetMetal` `Convert`; every
/// `Boolean::Apply.targets` and `Targets` in it, and its
/// `ShellFeature.bodies`, `Combine.target`, and `SplitMode::ByTools.targets`,
/// are `None`, and no `Boolean` in it is `NewBody`", and the rules on its
/// participants.
pub(super) fn check_assembly_feature(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    operation: &FeatureOperation,
    context: &AssemblyContext,
    findings: &mut Vec<Finding>,
) {
    if let Some(excluded) = excluded_operation(operation) {
        error(
            findings,
            site.entity,
            crate::rule!("design.definition-states-assembly-operation-assembly-feature", "a definition that states assembly has the operation {excluded}, which an assembly feature does not run"),
        );
    }
    if let Some(operand) = stated_body_operand(operation) {
        error(
            findings,
            site.entity,
            crate::rule!("design.definition-states-assembly-states-body-operand", "a definition that states assembly states a body operand of its own, {operand}; assembly.participants states what the feature changes"),
        );
    }
    if matches!(
        boolean_field(operation),
        BooleanField::Stated(Boolean::NewBody)
    ) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.definition-states-assembly-boolean-newbody",
                "a definition that states assembly has a Boolean that is NewBody"
            ),
        );
    }
    check_participants(site, context, findings);
    check_participant_operands(index, site, operation, context, findings);
}

/// Part 10 (§6.7): "Every pick of `AssemblyContext.participants` carries an
/// occurrence context. No selection in it is `Named` with no pick, a
/// selection with `Unread` picks is its only entry, no pick is in two of its
/// selections, and its selections state one `at`. An `Affected` term occurs
/// only among the `terms` of a selection of `AssemblyContext.participants`,
/// in at most one selection of a context; where that selection's `resolved`
/// is stated, it holds no participant another entry of the context states."
/// The seeds, propagation, and placement of an `Affected` term are checked
/// with every selection's rules.
fn check_participants(site: Site<'_, '_>, context: &AssemblyContext, findings: &mut Vec<Finding>) {
    let selections: Vec<&SetSelection> = context.participants.values().collect();
    let mut record = |message: Ruled<'_>| error(findings, site.entity, message);
    if selections
        .iter()
        .flat_map(|selection| participant_picks(selection, true))
        .any(|pick| !matches!(pick.context, Some(PickContext::Occurrence(_))))
    {
        record(crate::rule!(
            "design.assembly-participant-pick-carries-no-occurrence",
            "an assembly participant pick carries no occurrence context"
        ));
    }
    if selections
        .iter()
        .any(|selection| matches!(&selection.picks, Picks::Named(members) if members.as_slice().is_empty()))
    {
        record(crate::rule!("design.assembly-participant-selection-named-no-pick", 
            "an assembly participant selection is Named with no pick"
        ));
    }
    if selections.len() > 1
        && selections
            .iter()
            .any(|selection| matches!(selection.picks, Picks::Unread))
    {
        record(crate::rule!(
            "design.assembly-participant-selection-unread-picks-not",
            "an assembly participant selection with Unread picks is not its context's only entry"
        ));
    }
    if selections
        .iter()
        .any(|selection| selection.at != selections[0].at)
    {
        record(crate::rule!(
            "design.selections-assembly-context-state-more-one",
            "the selections of an assembly context state more than one at"
        ));
    }
    let mut held = BTreeSet::new();
    let mut repeated = false;
    for selection in &selections {
        let own: BTreeSet<_> = participant_picks(selection, false)
            .into_iter()
            .map(Keyed::key)
            .collect();
        for key in own {
            repeated |= !held.insert(key);
        }
    }
    if repeated {
        record(crate::rule!(
            "design.pick-two-selections-assembly-context",
            "a pick is in two selections of an assembly context"
        ));
    }
    let affected = |terms: &[crate::design::selection::RuleTerm<_>]| {
        terms
            .iter()
            .any(|term| matches!(term.rule, SelectionRule::Affected))
    };
    let mut holding = 0;
    for selection in &selections {
        if let Picks::Rule {
            terms, excluded, ..
        } = &selection.picks
        {
            if affected(excluded) {
                record(crate::rule!("design.affected-rule-term-among-excluded-terms", 
                    "an Affected rule term is among the excluded terms of an assembly participant selection"
                ));
            }
            if affected(terms) {
                holding += 1;
            }
        }
    }
    if holding > 1 {
        record(crate::rule!(
            "design.more-one-selection-assembly-context-holds",
            "more than one selection of an assembly context holds an Affected rule term"
        ));
    }
}

/// The picks a participant selection lists: its `Named` members, or its
/// terms' seeds and its cached result, with its excluded terms' seeds where
/// `excluded` is set.
fn participant_picks(selection: &SetSelection, excluded: bool) -> Vec<&Picked> {
    match &selection.picks {
        Picks::Unread => Vec::new(),
        Picks::Named(members) => members.as_slice().iter().collect(),
        Picks::Rule {
            terms,
            excluded: removed,
            resolved,
        } => terms
            .iter()
            .chain(removed.iter().filter(|_| excluded))
            .flat_map(|term| term.seeds.as_slice())
            .chain(resolved.iter().flat_map(Members::as_slice))
            .collect(),
    }
}

/// The name of `operation`, typed or `Unresolved` by its family, where §6.7
/// excludes it from an assembly feature.
fn excluded_operation(operation: &FeatureOperation) -> Option<&'static str> {
    use FeatureOperation as Op;
    use OperationFamily as Family;
    match operation {
        Op::Place(_)
        | Op::Unresolved {
            family: Family::Place,
        } => Some("Place"),
        Op::PlaceSet(_)
        | Op::Unresolved {
            family: Family::PlaceSet,
        } => Some("PlaceSet"),
        Op::Joint(_)
        | Op::Unresolved {
            family: Family::Joint,
        } => Some("Joint"),
        Op::RigidGroup(_)
        | Op::Unresolved {
            family: Family::RigidGroup,
        } => Some("RigidGroup"),
        Op::ComponentPattern(_)
        | Op::Unresolved {
            family: Family::ComponentPattern,
        } => Some("ComponentPattern"),
        Op::MirrorComponents(_)
        | Op::Unresolved {
            family: Family::MirrorComponents,
        } => Some("MirrorComponents"),
        Op::Stored { .. } => Some("Stored"),
        Op::DeleteBodies(_)
        | Op::Unresolved {
            family: Family::DeleteBodies,
        } => Some("DeleteBodies"),
        Op::TransformBodies(_)
        | Op::Unresolved {
            family: Family::TransformBodies,
        } => Some("TransformBodies"),
        Op::Flex(_)
        | Op::Unresolved {
            family: Family::Flex,
        } => Some("Flex"),
        Op::Graft(_)
        | Op::Unresolved {
            family: Family::Graft,
        } => Some("Graft"),
        Op::Knit(_)
        | Op::Unresolved {
            family: Family::Knit,
        } => Some("Knit"),
        Op::Mesh(_)
        | Op::Unresolved {
            family: Family::Mesh,
        } => Some("Mesh"),
        Op::Split(split) if matches!(split.mode, SplitMode::Mutual { .. }) => {
            Some("a Split in Mutual mode")
        }
        Op::SheetMetal(sheet) if matches!(sheet.kind, SheetMetalKind::Convert(_)) => {
            Some("a SheetMetal Convert")
        }
        _ => None,
    }
}

/// The field of `operation` that states bodies it changes, where §6.7
/// requires `None` in an assembly feature.
fn stated_body_operand(operation: &FeatureOperation) -> Option<&'static str> {
    if matches!(
        boolean_field(operation),
        BooleanField::Stated(Boolean::Apply {
            targets: Some(_),
            ..
        })
    ) {
        return Some("Boolean::Apply.targets");
    }
    match operation {
        FeatureOperation::Split(op)
            if matches!(
                op.mode,
                SplitMode::ByTools {
                    targets: Some(_),
                    ..
                }
            ) =>
        {
            Some("SplitMode::ByTools.targets")
        }
        FeatureOperation::Hole(op) if op.targets.is_some() => Some("Hole.targets"),
        FeatureOperation::Emboss(op) if op.targets.is_some() => Some("Emboss.targets"),
        FeatureOperation::Shell(op) if op.bodies.is_some() => Some("ShellFeature.bodies"),
        FeatureOperation::Combine(op) if op.target.is_some() => Some("Combine.target"),
        _ => None,
    }
}

/// `Feature.component` names a definition the feature's scope spans, and
/// only in a scope that spans several.
pub(super) fn check_component(
    index: &DesignRecords<'_>,
    feature: &Feature,
    findings: &mut Vec<Finding>,
) {
    let Some(component) = &feature.component else {
        return;
    };
    let Some(scope) = index.scope_of(MemberKey::Feature(feature.id.as_str())) else {
        return;
    };
    let spanned = index.spanned_definitions(scope);
    if spanned.len() < 2 || !spanned.contains(component.as_str()) {
        error(
            findings,
            feature.id.as_str(),
            crate::rule!(
                "design.feature-component-names-no-definition-scope",
                "Feature.component names {}, which is no definition its scope spans beside its own",
                component.as_str()
            ),
        );
    }
}

/// A feature with `driven_by` is in a scope other than its driver's.
pub(super) fn check_driven_by(
    index: &DesignRecords<'_>,
    feature: &Feature,
    findings: &mut Vec<Finding>,
) {
    let Some(Driver::Feature(driver)) = &feature.driven_by else {
        return;
    };
    let own = index.scope_of(MemberKey::Feature(feature.id.as_str()));
    let driver_scope = index.scope_of(MemberKey::Feature(driver.as_str()));
    if own.is_some() && own == driver_scope {
        error(
            findings,
            feature.id.as_str(),
            crate::rule!(
                "design.feature-driven-feature-own-scope",
                "the feature is driven by {}, a feature of its own scope",
                driver.as_str()
            ),
        );
    }
}

/// Every occurrence has at most one creating feature; two `Place`s of one
/// occurrence are members of one ordered container; no other combine,
/// component pattern, or mirror names the occurrence a `Combine` creates,
/// and the combine is before every placement that names it, in that
/// placement's container chain (§5.3).
pub(super) fn check_occurrence_creators(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let mut order: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
    for container in &index.ir.design.containers {
        if let Container::Ordered { id, members, .. } = container {
            for (at, member) in members.iter().enumerate() {
                if let MemberKey::Feature(feature) = MemberKey::of(member) {
                    order.entry(feature).or_insert((id.as_str(), at));
                }
            }
        }
    }
    let named = index.occurrence_namers();
    for (occurrence, namers) in &named {
        let places: Vec<&str> = namers
            .iter()
            .filter(|(_, _, place)| *place)
            .map(|(feature, _, _)| *feature)
            .collect();
        let containers: BTreeSet<Option<&str>> = places
            .iter()
            .map(|feature| order.get(feature).map(|(container, _)| *container))
            .collect();
        if places.len() > 1 && (containers.len() > 1 || containers.contains(&None)) {
            error(
                findings,
                places[0],
                crate::rule!(
                    "design.places-occurrence-not-members-one-ordered",
                    "Places of occurrence {occurrence} are not members of one ordered container"
                ),
            );
        }
        let placements: Vec<(&str, bool)> = namers
            .iter()
            .filter(|(_, namer, _)| *namer == Namer::Placement)
            .map(|(feature, _, place)| (*feature, *place))
            .collect();
        for (at, &(first, first_place)) in placements.iter().enumerate() {
            for &(second, second_place) in &placements[at + 1..] {
                if (!first_place || !second_place)
                    && ordered_pair(index, first, second) == Some(false)
                {
                    error(
                        findings,
                        second,
                        crate::rule!("design.placements-occurrence-not-ordered-one-container", 
                            "the placements {first} and {second} of occurrence {occurrence} are not ordered in one container chain"
                        ),
                    );
                }
            }
        }
        let combines: Vec<&str> = namers
            .iter()
            .filter(|(_, namer, _)| *namer == Namer::Combine)
            .map(|(feature, _, _)| *feature)
            .collect();
        for combine in &combines {
            let shared = namers.iter().any(|(other, namer, _)| {
                other != combine
                    && (*namer != Namer::Placement
                        || precedes(index, combine, other) == Some(false))
            });
            if shared {
                error(
                    findings,
                    combine,
                    crate::rule!("design.occurrence-combine-new-component-creates-named", "occurrence {occurrence}, which this combine's new_component creates, is named by another combine, pattern, or mirror, or by a placement the combine is not before in its container chain"),
                );
            }
        }
        let placed = namers
            .iter()
            .any(|(_, namer, _)| *namer == Namer::Placement);
        let creators = namers
            .iter()
            .filter(|(_, namer, _)| *namer == Namer::Creator)
            .count();
        if combines.is_empty() && !placed && creators > 1 {
            error(
                findings,
                occurrence,
                crate::rule!(
                    "design.occurrence-creating-features-most-one",
                    "occurrence has {creators} creating features; it has at most one"
                ),
            );
        }
    }
}

/// Part 10 (§6.7): "`OwnOccurrence` occurs only where the occurrence's
/// prototype has an occurrence of its own in the scope": another occurrence
/// of the prototype, owned by a definition the scope spans.
fn check_own_occurrence(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    place: &Place,
    findings: &mut Vec<Finding>,
) {
    if !matches!(place.base, Some(PlacementBase::OwnOccurrence)) {
        return;
    }
    let (Some(prototype), Some(scope)) = (index.prototype(place.occurrence.as_str()), site.scope)
    else {
        return;
    };
    let spanned = index.spanned_definitions(scope);
    let own = index.occurrences.iter().any(|(&id, occurrence)| {
        id != place.occurrence.as_str()
            && occurrence.prototype.as_str() == prototype
            && owner(index, id).is_some_and(|owner| spanned.contains(owner))
    });
    if !own {
        error(
            findings,
            site.entity,
            crate::rule!("design.place-base-ownoccurrence-no-occurrence-own", 
                "Place.base is OwnOccurrence, but {prototype} has no occurrence of its own in the scope"
            ),
        );
    }
}

/// The containers above a feature, innermost first, each with the index of
/// the member on the way down. `None` where the feature is in no container
/// or the parents do not end.
fn container_chain<'a>(index: &DesignRecords<'a>, feature: &str) -> Option<Vec<(&'a str, usize)>> {
    let mut chain = Vec::new();
    let mut member = MemberKey::Feature(feature);
    for _ in 0..=index.containers.len() {
        let Some(parent) = index.parent_of(member) else {
            return (!chain.is_empty()).then_some(chain);
        };
        let container = index.containers.get(parent)?;
        let at = super::super::records::members(container)
            .into_iter()
            .position(|candidate| MemberKey::of(candidate) == member)?;
        chain.push((parent, at));
        member = MemberKey::Container(parent);
    }
    None
}

/// Whether one of two features is before the other in the other's container
/// chain (§5.3): their innermost common container is ordered. `None` where
/// either chain is not known.
fn ordered_pair(index: &DesignRecords<'_>, first: &str, second: &str) -> Option<bool> {
    let first = container_chain(index, first)?;
    let second = container_chain(index, second)?;
    let common = first
        .iter()
        .find(|(container, _)| second.iter().any(|(other, _)| other == container));
    Some(common.is_some_and(|(container, _)| {
        matches!(
            index.containers.get(container),
            Some(Container::Ordered { .. })
        )
    }))
}

/// Whether `first` is before `second` in `second`'s container chain (§5.3):
/// their innermost common container is ordered and lists `first`'s branch
/// first. `None` where either chain is not known.
fn precedes(index: &DesignRecords<'_>, first: &str, second: &str) -> Option<bool> {
    let first = container_chain(index, first)?;
    let second = container_chain(index, second)?;
    let common = first.iter().find_map(|(container, at)| {
        second
            .iter()
            .find(|(other, _)| other == container)
            .map(|(_, other_at)| (*container, *at, *other_at))
    });
    Some(common.is_some_and(|(container, at, other_at)| {
        matches!(
            index.containers.get(container),
            Some(Container::Ordered { .. })
        ) && at < other_at
    }))
}

/// Part 10 (§6.7): "A `Coupling` whose `joints` is absent has connectors that
/// each pick a member it couples." A connector with no input picks nothing.
fn check_coupling_connectors(site: Site<'_, '_>, joint: &Joint, findings: &mut Vec<Finding>) {
    if !matches!(joint.kind, JointKind::Coupling { joints: None, .. }) {
        return;
    }
    let picks_nothing = joint.connectors.iter().any(|connector| match connector {
        JointConnector::Frame(c) => c.inputs.is_empty(),
        JointConnector::Plane(c) => c.inputs.is_empty(),
        JointConnector::Axis(c) => c.inputs.is_empty(),
        JointConnector::Point(c) => c.inputs.is_empty(),
        JointConnector::Entity(_) => false,
    });
    if picks_nothing {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.coupling-no-joints-connector-picks-no",
                "a Coupling with no joints has a connector that picks no member"
            ),
        );
    }
}

/// The occurrence path a participant pick names.
fn pick_path(pick: &Picked) -> Option<&OccurrencePath> {
    match &pick.context {
        Some(
            PickContext::Occurrence(path)
            | PickContext::InContext {
                target: Some(path), ..
            },
        ) => Some(path),
        _ => None,
    }
}

/// The participants of a selection, as the picks that list them: its
/// `Named` members, or a rule's stated `resolved`. `None` where the
/// selection does not list them (§6.7: such a rule is not checked).
fn listed_participants(selection: &SetSelection) -> Option<&[Picked]> {
    match &selection.picks {
        Picks::Unread => None,
        Picks::Named(members) => Some(members.as_slice()),
        Picks::Rule { resolved, .. } => resolved.as_ref().map(Members::as_slice),
    }
}

/// The paths of the participants the selections under `effect` list, where
/// every selection of the context lists them; `None` otherwise.
fn participant_paths(
    context: &AssemblyContext,
    effect: impl Fn(Option<&ContextEffect>) -> bool,
) -> Option<BTreeSet<&OccurrencePath>> {
    let mut paths = BTreeSet::new();
    for (key, selection) in &context.participants {
        let listed = listed_participants(selection)?;
        if effect(key.as_ref()) {
            paths.extend(listed.iter().filter_map(pick_path));
        }
    }
    Some(paths)
}

/// The prototype of a path's last occurrence.
fn path_prototype<'a>(index: &DesignRecords<'a>, path: &OccurrencePath) -> Option<&'a str> {
    match &path.0.last()?.occurrence {
        StepRef::Occurrence(occurrence) => index.prototype(occurrence.as_str()),
        StepRef::Native(_) => None,
    }
}

/// Part 10 (§6.5, §6.7): a hole series' parts and a split's piece picks are
/// participants. "Each `SeriesStage.parts` pick resolves to a member of one of
/// that context's selections"; a `PartFeature` stage's feature "is a `Hole`
/// of that part's definition; that definition has one occurrence among the
/// members of the context's selections"; "a piece pick targets an entity ...
/// of a participant where `targets` is `None`". A context whose selections
/// do not list their participants is not checked.
fn check_participant_operands(
    index: &DesignRecords<'_>,
    site: Site<'_, '_>,
    operation: &FeatureOperation,
    context: &AssemblyContext,
    findings: &mut Vec<Finding>,
) {
    let Some(members) = participant_paths(context, |_| true) else {
        return;
    };
    match operation {
        FeatureOperation::Hole(hole) => {
            for stage in &hole.series {
                let Some(parts) = picks::named(&stage.parts) else {
                    continue;
                };
                for path in parts.iter().filter_map(pick_path) {
                    if !members.contains(path) {
                        error(
                            findings,
                            site.entity,
                            crate::rule!(
                                "design.series-stage-picks-part-no-participant",
                                "a series stage picks a part that is no participant"
                            ),
                        );
                    }
                }
                let StageHole::PartFeature(feature) = &stage.hole else {
                    continue;
                };
                let [part] = parts else {
                    continue;
                };
                let Some(definition) = pick_path(part).and_then(|path| path_prototype(index, path))
                else {
                    continue;
                };
                let held_by = index.features.get(feature.as_str()).and_then(|record| {
                    record
                        .component
                        .as_ref()
                        .map(crate::ids::ProductDefinitionId::as_str)
                        .or_else(|| index.scope_of(MemberKey::Feature(feature.as_str())))
                });
                if held_by.is_some_and(|held_by| held_by != definition) {
                    error(
                        findings,
                        site.entity,
                        crate::rule!("design.stagehole-partfeature-names-no-feature-part", 
                            "StageHole::PartFeature names {}, which is no feature of the part's definition {definition}",
                            feature.as_str()
                        ),
                    );
                }
                let placed = members
                    .iter()
                    .filter(|path| path_prototype(index, path) == Some(definition))
                    .count();
                if placed != 1 {
                    error(
                        findings,
                        site.entity,
                        crate::rule!("design.part-partfeature-stage-occurrences-among-participants", 
                            "{definition}, the part of a PartFeature stage, has {placed} occurrences among the participants; it has one"
                        ),
                    );
                }
            }
        }
        FeatureOperation::Split(split) => {
            let SplitMode::ByTools {
                targets: None,
                kept: Some(Kept::Pieces { keep, remove }),
                ..
            } = &split.mode
            else {
                return;
            };
            let owner = owner_definition(site);
            let participants: Option<Vec<_>> = context
                .participants
                .values()
                .filter_map(listed_participants)
                .flatten()
                .map(|pick| Rooted::of(pick, owner))
                .collect();
            let Some(participants) = participants else {
                return;
            };
            for list in [keep, remove].into_iter().flatten() {
                let Some(pieces) = picks::named(list) else {
                    continue;
                };
                for piece in pieces.iter().filter_map(|pick| Rooted::of(pick, owner)) {
                    let off = participants
                        .iter()
                        .all(|member| piece.under(member, owner) == Some(false));
                    if off {
                        error(
                            findings,
                            site.entity,
                            crate::rule!(
                                "design.piece-pick-targets-entity-no-participant",
                                "a piece pick targets an entity of no participant"
                            ),
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

/// Part 10 (§6.7): "A feature whose `driven_by` names an assembly feature that
/// holds no `Native` participants is a feature of the definition one of its
/// `Definitions` participants resolves in." The definitions the driver's
/// configuration overrides state are read with its own; a context with an
/// unstated effect, or whose selections do not list their participants, is
/// not checked.
pub(super) fn check_driven_definition(
    index: &DesignRecords<'_>,
    feature: &Feature,
    findings: &mut Vec<Finding>,
) {
    let Some(Driver::Feature(driver)) = &feature.driven_by else {
        return;
    };
    let Some(record) = index.features.get(driver.as_str()) else {
        return;
    };
    let design = &index.ir.design;
    let overrides = design
        .configurations
        .iter()
        .map(|configuration| &configuration.overrides)
        .chain(
            design
                .arrangements
                .iter()
                .map(|arrangement| &arrangement.overrides),
        )
        .filter_map(|overrides| overrides.features.get(driver)?.definition.as_ref());
    let mut targets = BTreeSet::new();
    let mut stated = false;
    for definition in std::iter::once(&record.definition).chain(overrides) {
        let Some(context) = &definition.assembly else {
            continue;
        };
        stated = true;
        if context
            .participants
            .keys()
            .any(|key| matches!(key, None | Some(ContextEffect::Native(_))))
        {
            return;
        }
        let Some(paths) = participant_paths(context, |key| {
            matches!(key, Some(ContextEffect::Definitions))
        }) else {
            return;
        };
        targets.extend(
            paths
                .into_iter()
                .filter_map(|path| path_prototype(index, path)),
        );
    }
    let own = feature
        .component
        .as_ref()
        .map(crate::ids::ProductDefinitionId::as_str)
        .or_else(|| index.scope_of(MemberKey::Feature(feature.id.as_str())));
    if let (true, Some(own)) = (stated, own) {
        if !targets.contains(own) {
            error(
                findings,
                feature.id.as_str(),
                crate::rule!("design.feature-driven-definitions-participants-resolve-no", 
                    "the feature is driven by {}, whose Definitions participants resolve in no definition it belongs to",
                    driver.as_str()
                ),
            );
        }
    }
}

/// Part 10 (§5.1): "No definition is spanned by two scopes."
pub(super) fn check_spans(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let mut spanned_by: BTreeMap<&str, &str> = BTreeMap::new();
    for scope in &index.ir.design.scopes {
        let own = scope.definition.as_str();
        for definition in index.spanned_definitions(own).iter().copied() {
            if let Some(first) = spanned_by.insert(definition, own) {
                if first != own {
                    error(
                        findings,
                        own,
                        crate::rule!(
                            "design.scope-spans-scope-spans",
                            "the scope spans {definition}, which the scope of {first} spans"
                        ),
                    );
                }
            }
        }
    }
}
