// SPDX-License-Identifier: Apache-2.0
//! Assembly operations (§6.7).
//!
//! An occurrence is owned by the definition that places it. Until product
//! definitions list their occurrences, a child occurrence is owned by its
//! parent occurrence's prototype, and a root occurrence's owner is not
//! decided here, so no ownership rule fires on it.

use std::collections::{BTreeMap, BTreeSet};

use super::picks::{self, Class};
use super::{body, error, Site};
use crate::design::construction::FrameAxis;
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::feature::{Driver, Feature, FeatureOperation};
use crate::design::ops::assembly::{
    AssemblyFeature, ComponentPattern, MirrorComponents, Place, PlaceSet, RigidGroup,
};
use crate::design::ops::shared::{
    AxisLimits, Bounds, Joint, JointConnector, JointKind, JointMotion, PathAxisControl,
    PathRollControl,
};
use crate::design::selection::{PickContext, PickTarget, StepRef};
use crate::products::OccurrenceParent;
use crate::report::check::Finding;

use super::super::records::{DesignRecords, MemberKey};

/// The definition that owns an occurrence, where its parent decides it.
fn owner<'a>(index: &DesignRecords<'a>, occurrence: &str) -> Option<&'a str> {
    match &index.occurrences.get(occurrence)?.parent {
        OccurrenceParent::Occurrence { occurrence: parent } => index.prototype(parent.as_str()),
        OccurrenceParent::Root {} => None,
    }
}

/// An occurrence a feature names exists, and its owner, where decided, is a
/// definition the feature's scope spans.
fn check_owned(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    field: &str,
    occurrence: &str,
    findings: &mut Vec<Finding>,
) {
    if !index.occurrences.contains_key(occurrence) {
        error(
            findings,
            site.entity,
            format_args!("{field} names {occurrence}, which is no occurrence of the model"),
        );
        return;
    }
    let (Some(scope), Some(owner)) = (site.scope, owner(index, occurrence)) else {
        return;
    };
    if !index.spanned_definitions(scope).contains(owner) {
        error(
            findings,
            site.entity,
            format_args!("{field} names {occurrence}, owned by {owner}, which the feature's scope does not span"),
        );
    }
}

pub(super) fn check_place(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    place: &Place,
    findings: &mut Vec<Finding>,
) {
    let occurrence = place.occurrence.as_str();
    check_owned(index, site, "Place.occurrence", occurrence, findings);
    if place.base.is_some() != place.placement.is_some() {
        error(
            findings,
            site.entity,
            format_args!("Place.base is present where placement is not, or absent where it is"),
        );
    }
    for constraint in &place.constraints {
        check_joint(site, constraint, findings);
        let picks_into = constraint
            .connectors
            .iter()
            .any(|connector| super::wire_strings(connector).contains(occurrence));
        if !picks_into {
            error(
                findings,
                site.entity,
                format_args!("a Place constraint has no connector that picks into {occurrence}"),
            );
        }
    }
}

pub(super) fn check_place_set(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    set: &PlaceSet,
    findings: &mut Vec<Finding>,
) {
    let distinct: BTreeSet<&str> = set.occurrences.iter().map(|o| o.as_str()).collect();
    let least = if set.native_options.is_empty() { 2 } else { 1 };
    if set.occurrences.len() < least || distinct.len() != set.occurrences.len() {
        error(
            findings,
            site.entity,
            format_args!("a PlaceSet lists an occurrence twice, or fewer than {least}"),
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

fn check_bounds<Q: Copy>(
    site: Site<'_>,
    bounds: &Bounds<Q>,
    value: impl Fn(Q) -> f64,
    findings: &mut Vec<Finding>,
) {
    if bounds.min.is_none() && bounds.max.is_none() {
        error(findings, site.entity, format_args!("Bounds holds no bound"));
    }
    if let (Some(Dimension::Literal(min)), Some(Dimension::Literal(max))) =
        (&bounds.min, &bounds.max)
    {
        if value(*min) > value(*max) {
            error(
                findings,
                site.entity,
                format_args!("Bounds has min above max"),
            );
        }
    }
}

/// A joint's connectors fit its kind; its motion's limits name free axes.
pub(super) fn check_joint(site: Site<'_>, joint: &Joint, findings: &mut Vec<Finding>) {
    check_connectors(site, &joint.connectors, &joint.kind, findings);
    match &joint.kind {
        JointKind::Motion { motion, limits, .. } => {
            if let JointMotion::PinSlot { axis, slide } = motion {
                if axis == slide {
                    error(
                        findings,
                        site.entity,
                        format_args!("a PinSlot's slide is its axis"),
                    );
                }
            }
            let free = free_axes(motion);
            let mut named = BTreeSet::new();
            for limit in limits {
                let (rotation, axis) = match limit {
                    AxisLimits::Rotation { axis, limits, .. } => {
                        if let Some(bounds) = limits {
                            check_bounds(site, bounds, |angle| angle.get(), findings);
                        }
                        (true, *axis)
                    }
                    AxisLimits::Translation { axis, limits, .. } => {
                        if let Some(bounds) = limits {
                            check_bounds(site, bounds, |length| length.get(), findings);
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
                        format_args!(
                            "AxisLimits names a degree of freedom the motion does not leave"
                        ),
                    );
                }
                if !named.insert((rotation, axis)) {
                    error(
                        findings,
                        site.entity,
                        format_args!("AxisLimits names one axis twice"),
                    );
                }
            }
        }
        JointKind::Distance {
            limits: Some(bounds),
            ..
        } => check_bounds(site, bounds, |length| length.get(), findings),
        JointKind::Angle {
            limits: Some(bounds),
            ..
        } => check_bounds(site, bounds, |angle| angle.get(), findings),
        JointKind::Path {
            pitch_yaw: Some(PathAxisControl::FollowPath { axis: pitch, .. }),
            roll: Some(PathRollControl::UpVector { axis: roll, .. }),
            ..
        } if pitch == roll => error(
            findings,
            site.entity,
            format_args!("a Path joint's pitch_yaw and roll name one axis"),
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
                    format_args!("Coupling.joints picks other than two other Joint features"),
                );
            }
        }
        _ => {}
    }
}

/// The connector variants a joint kind admits, and a world connector's
/// resolved value.
pub(super) fn check_connectors(
    site: Site<'_>,
    connectors: &[JointConnector; 2],
    kind: &JointKind,
    findings: &mut Vec<Finding>,
) {
    for connector in connectors {
        let world = match connector {
            JointConnector::Frame(c) => c.inputs.is_empty() && c.resolved.is_none(),
            JointConnector::Plane(c) => c.inputs.is_empty() && c.resolved.is_none(),
            JointConnector::Axis(c) => c.inputs.is_empty() && c.resolved.is_none(),
            JointConnector::Point(c) => c.inputs.is_empty() && c.resolved.is_none(),
            JointConnector::Entity(_) => false,
        };
        if world {
            error(
                findings,
                site.entity,
                format_args!("a connector with no inputs has no resolved value"),
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
            format_args!("the joint's connectors are of a variant its kind does not take"),
        );
    }
}

pub(super) fn check_rigid_group(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    group: &RigidGroup,
    findings: &mut Vec<Finding>,
) {
    if group.members.len() < 2 {
        error(
            findings,
            site.entity,
            format_args!("a RigidGroup has fewer than two members"),
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
                format_args!("a RigidGroup member path has a step other than an Occurrence"),
            );
        }
        if let Some(StepRef::Occurrence(first)) = path.0.first().map(|step| &step.occurrence) {
            check_owned(index, site, "RigidGroup.members", first.as_str(), findings);
        }
    }
}

fn check_component_seeds(
    site: Site<'_>,
    field: &str,
    seeds: &crate::design::selection::OrderedSelection,
    findings: &mut Vec<Finding>,
) {
    let allowed = |class| class == Class::Component;
    if let Some(class) = picks::refused(seeds, allowed) {
        error(
            findings,
            site.entity,
            format_args!("{field} picks {class}; it picks only Component targets"),
        );
    }
}

pub(super) fn check_component_pattern(
    index: &DesignRecords<'_>,
    site: Site<'_>,
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
                    format_args!(
                        "ChainLinks::Connected holds {} links for {seeds} seeds",
                        links.len()
                    ),
                );
            }
        }
    }
    let indices: Vec<_> = pattern
        .instances
        .iter()
        .map(|instance| &instance.index)
        .collect();
    body::check_indices(site, &pattern.layout, &indices, findings);
    let mut seen = BTreeSet::new();
    for instance in &pattern.instances {
        if seeds.is_some_and(|seeds| instance.occurrences.len() != seeds) {
            error(
                findings,
                site.entity,
                format_args!(
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
                    format_args!(
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

pub(super) fn check_mirror(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    mirror: &MirrorComponents,
    findings: &mut Vec<Finding>,
) {
    check_component_seeds(site, "MirrorComponents.seeds", &mirror.seeds, findings);
    if picks::count(&mirror.seeds).is_some_and(|seeds| seeds != mirror.results.len()) {
        error(
            findings,
            site.entity,
            format_args!("MirrorComponents.results has other than one result per seed"),
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

pub(super) fn check_assembly_feature(
    site: Site<'_>,
    feature: &AssemblyFeature,
    findings: &mut Vec<Finding>,
) {
    if matches!(
        *feature.operation,
        FeatureOperation::Place(_)
            | FeatureOperation::Joint(_)
            | FeatureOperation::RigidGroup(_)
            | FeatureOperation::ComponentPattern(_)
            | FeatureOperation::MirrorComponents(_)
            | FeatureOperation::AssemblyFeature(_)
            | FeatureOperation::Stored { .. }
    ) {
        error(
            findings,
            site.entity,
            format_args!("an AssemblyFeature's operation is an assembly operation or Stored"),
        );
    }
    if let Some(participants) = picks::named(&feature.participants) {
        if participants
            .iter()
            .any(|pick| !matches!(pick.context, Some(PickContext::Occurrence(_))))
        {
            error(
                findings,
                site.entity,
                format_args!("an AssemblyFeature participant carries no occurrence context"),
            );
        }
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
            format_args!(
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
            format_args!(
                "the feature is driven by {}, a feature of its own scope",
                driver.as_str()
            ),
        );
    }
}

/// Who names an occurrence.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Namer {
    Combine,
    Placement,
    Creator,
}

/// Every occurrence has at most one creating feature; two `Place`s of one
/// occurrence are members of one ordered container; no other combine,
/// component pattern, or mirror names the occurrence a `Combine` creates,
/// and no placement before the combine names it.
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
    let before = |a: &str, b: &str| matches!((order.get(a), order.get(b)), (Some((ca, ia)), Some((cb, ib))) if ca == cb && ia < ib);
    let mut named: BTreeMap<&str, Vec<(&str, Namer, bool)>> = BTreeMap::new();
    for feature in &index.ir.design.features {
        let id = feature.id.as_str();
        let found: Vec<(&str, Namer, bool)> = match &feature.definition.operation {
            FeatureOperation::Combine(combine) => combine
                .new_component
                .iter()
                .map(|new| (new.occurrence.as_str(), Namer::Combine, false))
                .collect(),
            FeatureOperation::Place(place) => {
                vec![(place.occurrence.as_str(), Namer::Placement, true)]
            }
            FeatureOperation::PlaceSet(set) => set
                .occurrences
                .iter()
                .map(|occurrence| (occurrence.as_str(), Namer::Placement, false))
                .collect(),
            FeatureOperation::ComponentPattern(pattern) => pattern
                .instances
                .iter()
                .flat_map(|instance| &instance.occurrences)
                .map(|occurrence| (occurrence.as_str(), Namer::Creator, false))
                .collect(),
            FeatureOperation::MirrorComponents(mirror) => mirror
                .results
                .iter()
                .map(|result| (result.occurrence.as_str(), Namer::Creator, false))
                .collect(),
            _ => Vec::new(),
        };
        for (occurrence, namer, place) in found {
            named
                .entry(occurrence)
                .or_default()
                .push((id, namer, place));
        }
    }
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
                format_args!(
                    "Places of occurrence {occurrence} are not members of one ordered container"
                ),
            );
        }
        let combines: Vec<&str> = namers
            .iter()
            .filter(|(_, namer, _)| *namer == Namer::Combine)
            .map(|(feature, _, _)| *feature)
            .collect();
        for combine in &combines {
            let shared = namers.iter().any(|(other, namer, _)| {
                other != combine && (*namer != Namer::Placement || before(other, combine))
            });
            if shared {
                error(
                    findings,
                    combine,
                    format_args!("occurrence {occurrence}, which this combine's new_component creates, is named by another combine, pattern, or mirror, or placed before the combine"),
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
                format_args!("occurrence has {creators} creating features; it has at most one"),
            );
        }
    }
}
