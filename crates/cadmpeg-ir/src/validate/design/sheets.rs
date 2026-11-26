// SPDX-License-Identifier: Apache-2.0
//! The sheet-metal bodies of a state and the rules that govern them (§6.6),
//! read one configuration, or one arrangement over a configuration, at a
//! time, with the definition and suppression in force there: whether the
//! state entering a feature holds a sheet-metal body, the literal thickness
//! of each sheet where the features before it keep every sheet's thickness
//! edges, an `Extrude.sheet_cut` cuts a sheet-metal body, and each body has
//! at most one governing rule.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::configuration::Arrangement;
use crate::design::container::Container;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ids::SheetMetalRuleId;
use crate::design::ops::body::{DeleteBodies, Retention};
use crate::design::ops::sheet_metal::{Flange, Hem, SheetMetalKind};
use crate::design::options::{Boolean, BooleanOp, Thickness};
use crate::design::refs::Member;
use crate::design::selection::{PickTarget, Picks, SetSelection};
use crate::report::check::Finding;

use super::index::{DesignIndex, StateAt};
use super::ops::picks::share_pick;
use super::picked::literal;
use super::records::MemberKey;

/// The rule a sheet-metal kind names.
pub(super) const fn named_rule(kind: &SheetMetalKind) -> Option<&SheetMetalRuleId> {
    match kind {
        SheetMetalKind::ApplyRule { rule, .. } => Some(rule),
        SheetMetalKind::Wall { rule, .. }
        | SheetMetalKind::ContourFlange { rule, .. }
        | SheetMetalKind::ExtrudedHole { rule, .. }
        | SheetMetalKind::Stamp { rule, .. }
        | SheetMetalKind::ToolForm { rule, .. } => rule.as_ref(),
        SheetMetalKind::Flange(Flange { rule, .. }) | SheetMetalKind::Hem(Hem { rule, .. }) => {
            rule.as_ref()
        }
        SheetMetalKind::Convert(convert) => convert.rule.as_ref(),
        _ => None,
    }
}

/// The sheet-metal bodies of the state entering a feature in one
/// configuration.
pub(super) struct Entering {
    /// Whether the state can hold a sheet-metal body: a sheet-metal feature
    /// of the state has a result that no `DeleteBodies` of the state removes
    /// whole, or that a feature of the state can change.
    pub(super) sheet_metal: bool,
    /// The literal thicknesses the sheet-metal features state, where every
    /// feature of the state keeps each sheet's thickness edges whole.
    pub(super) thicknesses: Option<Vec<f64>>,
}

/// What an operation does to the sheet-metal bodies of a state.
enum Effect<'a> {
    /// It changes no body.
    Keeps,
    /// It can reshape, copy, or split a body and makes no sheet-metal body.
    Reshapes,
    /// It removes bodies.
    Deletes(&'a DeleteBodies),
    /// It is a sheet-metal feature, with the literal thicknesses it states.
    Sheet(Option<Vec<f64>>),
    /// Its effect is not read.
    Unknown,
}

/// One reading of a scope's features: a configuration, and an arrangement
/// whose overrides resolve over it (§5.1).
#[derive(Clone, Copy)]
pub(super) struct Reading<'a> {
    configuration: Option<&'a str>,
    arrangement: Option<&'a Arrangement>,
}

impl<'a> Reading<'a> {
    /// The reading of a configuration, or of the base, with no arrangement.
    pub(super) const fn of(configuration: Option<&'a str>) -> Self {
        Self {
            configuration,
            arrangement: None,
        }
    }

    /// Whether a feature is suppressed; `None` where that is a dimension
    /// that is not literal (§10, Bounded values).
    fn suppressed(self, index: &DesignIndex<'a>, feature: &Feature) -> Option<bool> {
        match index
            .records
            .feature_overrides(&feature.id, self.configuration, self.arrangement)
            .into_iter()
            .find_map(|stated| stated.suppressed)
        {
            Some(suppressed) => Some(suppressed),
            None => feature.suppressed.as_ref().map_or(Some(false), literal),
        }
    }
}

/// The sheet-metal bodies entering `feature`, a member of an ordered
/// container, where each feature of the state has the definition and
/// suppression in force in `reading`. `None` where a feature of the state
/// has an effect that is not read, or the definition takes bodies from
/// elsewhere.
pub(super) fn entering<'a>(
    index: &DesignIndex<'a>,
    feature: &'a str,
    reading: Reading<'a>,
) -> Option<Entering> {
    index.entering(feature)?;
    let owner = index.owner_definition(feature)?;
    let definition = index.records.definitions.get(owner.as_str())?;
    if definition.generated_from.is_some()
        || !definition.occurrences.is_empty()
        || !definition.instance_bodies.is_empty()
    {
        return None;
    }
    let mut thicknesses = Some(Vec::new());
    let mut sheets = Vec::new();
    let mut reshapers = Vec::new();
    // The `DeleteBodies` features in force that remove each sheet whole.
    let mut deleters: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for &id in index.state_features(StateAt::Entering(feature))?.iter() {
        let record = *index.records.features.get(id)?;
        let suppressed = reading.suppressed(index, record);
        if suppressed == Some(true) {
            continue;
        }
        match effect(
            index,
            &index
                .records
                .definition_in(record, reading.configuration, reading.arrangement)
                .operation,
        ) {
            Effect::Keeps => {}
            Effect::Reshapes => {
                thicknesses = None;
                reshapers.push(id);
            }
            Effect::Deletes(delete) => {
                thicknesses = None;
                if suppressed == Some(false) {
                    for sheet in whole_results(delete) {
                        deleters.entry(sheet).or_default().push(id);
                    }
                }
            }
            Effect::Sheet(found) => {
                sheets.push(id);
                match (&mut thicknesses, found, suppressed) {
                    (Some(all), Some(found), Some(false)) => all.extend(found),
                    (all, _, _) => *all = None,
                }
            }
            Effect::Unknown => return None,
        }
    }
    // Whether `earlier` is in the state entering `feature`, taken as so where
    // that state is not read.
    let before = |earlier: &str, feature: &'a str| {
        index
            .state_features(StateAt::Entering(feature))
            .is_none_or(|state| state.contains(earlier))
    };
    // A sheet survives where no delete removes it whole, or where a feature
    // that runs after it and before every delete of it can copy or reshape
    // it.
    let sheet_metal = sheets.iter().any(|&sheet| match deleters.get(sheet) {
        None => true,
        Some(deletes) => reshapers.iter().any(|&reshaper| {
            before(sheet, reshaper) && deletes.iter().all(|&delete| before(reshaper, delete))
        }),
    });
    Some(Entering {
        sheet_metal,
        thicknesses,
    })
}

/// The features whose whole results a `DeleteBodies` removes: those its
/// `FeatureResult` picks name with no narrowing `resolved` list.
fn whole_results(delete: &DeleteBodies) -> Vec<&str> {
    let Picks::Named(picks) = &delete.bodies.picks else {
        return Vec::new();
    };
    if delete.retain != Some(Retention::DeleteSelected) {
        return Vec::new();
    }
    picks
        .as_slice()
        .iter()
        .filter_map(|pick| match (&pick.context, &pick.target) {
            (None, PickTarget::FeatureResult(feature)) if pick.resolved.is_none() => {
                Some(feature.as_str())
            }
            _ => None,
        })
        .collect()
}

/// The readings a member's features are judged in: the base and each
/// configuration of its scope, and each arrangement of the scope that
/// overrides a feature's definition or suppression over each configuration
/// it applies in.
fn readings<'a>(index: &DesignIndex<'a>, member: MemberKey<'a>) -> Vec<Reading<'a>> {
    let records = &index.records;
    let scope = records.scope_of(member);
    let configurations: Vec<Option<&'a str>> = std::iter::once(None)
        .chain(
            scope
                .and_then(|scope| records.scope_configurations.get(scope))
                .into_iter()
                .flatten()
                .map(|configuration| Some(configuration.id.as_str())),
        )
        .collect();
    let mut readings: Vec<Reading<'a>> = configurations.iter().copied().map(Reading::of).collect();
    let arrangements = records
        .arrangements
        .values()
        .copied()
        .filter(|arrangement| {
            Some(arrangement.scope.as_str()) == scope
                && arrangement
                    .overrides
                    .features
                    .values()
                    .any(|stated| stated.definition.is_some() || stated.suppressed.is_some())
        });
    for arrangement in arrangements {
        let applies: Vec<Option<&'a str>> = match &arrangement.configuration {
            Some(configuration) => vec![Some(configuration.as_str())],
            None => configurations.clone(),
        };
        readings.extend(applies.into_iter().map(|configuration| Reading {
            configuration,
            arrangement: Some(arrangement),
        }));
    }
    readings
}

fn effect<'a>(index: &DesignIndex<'_>, operation: &'a FeatureOperation) -> Effect<'a> {
    match operation {
        FeatureOperation::Sketch(_)
        | FeatureOperation::SpatialSketch(_)
        | FeatureOperation::Spreadsheet(_)
        | FeatureOperation::Datum(_)
        | FeatureOperation::Curve(_) => Effect::Keeps,
        FeatureOperation::DeleteBodies(delete) => Effect::Deletes(delete),
        FeatureOperation::Extrude(_)
        | FeatureOperation::Revolve(_)
        | FeatureOperation::Sweep(_)
        | FeatureOperation::HelicalSweep(_)
        | FeatureOperation::Rib(_)
        | FeatureOperation::Primitive(_)
        | FeatureOperation::Loft(_)
        | FeatureOperation::Fillet(_)
        | FeatureOperation::Chamfer(_)
        | FeatureOperation::Hole(_)
        | FeatureOperation::Thread(_)
        | FeatureOperation::Emboss(_)
        | FeatureOperation::Dome(_)
        | FeatureOperation::Decal(_)
        | FeatureOperation::Pattern(_)
        | FeatureOperation::Combine(_)
        | FeatureOperation::Split(_)
        | FeatureOperation::Extract(_)
        | FeatureOperation::TransformBodies(_)
        | FeatureOperation::MaterialRemoval => Effect::Reshapes,
        FeatureOperation::SheetMetal(sheet) => Effect::Sheet(thicknesses(index, &sheet.kind)),
        _ => Effect::Unknown,
    }
}

/// The literal thicknesses a sheet-metal kind and the rule it names state.
/// `None` where one is not literal, or where the kind can change a sheet's
/// thickness edges in a way that is not read.
fn thicknesses(index: &DesignIndex<'_>, kind: &SheetMetalKind) -> Option<Vec<f64>> {
    let rule = named_rule(kind);
    let mut found = Vec::new();
    if let Some(rule) = rule {
        let record = index
            .records
            .ir
            .design
            .sheet_metal_rules
            .iter()
            .find(|record| record.id == *rule)?;
        found.push(literal(record.thickness.as_ref()?)?.get());
    }
    match kind {
        SheetMetalKind::Wall { thickness, .. } => match thickness {
            Some(thickness) => found.push(literal(thickness)?.get()),
            None => {
                rule?;
            }
        },
        SheetMetalKind::ContourFlange { thickness, .. } => match thickness {
            Some(thickness) => found.push(whole(thickness)?),
            None => {
                rule?;
            }
        },
        SheetMetalKind::Convert(convert) => found.push(literal(&convert.thickness)?.get()),
        SheetMetalKind::Stamp { .. }
        | SheetMetalKind::ToolForm { .. }
        | SheetMetalKind::Connect { .. }
        | SheetMetalKind::Native(_) => return None,
        _ => {}
    }
    Some(found)
}

/// The whole thickness a literal `Thickness` states.
fn whole(thickness: &Thickness) -> Option<f64> {
    match thickness {
        Thickness::OneSide { value, .. } | Thickness::Symmetric(value) => {
            Some(literal(value)?.get())
        }
        Thickness::Offsets { outside, inside } => {
            let total = literal(outside)?.get() + literal(inside)?.get();
            (total > 0.0).then_some(total)
        }
        Thickness::Native(_) => None,
    }
}

/// Whether a definition is an extrude that cuts with `sheet_cut`.
fn sheet_cut(definition: &FeatureDefinition) -> bool {
    matches!(
        &definition.operation,
        FeatureOperation::Extrude(extrude)
            if extrude.sheet_cut.is_some()
                && matches!(
                    extrude.output.boolean,
                    Some(Boolean::Apply { op: BooleanOp::Cut, .. })
                )
    )
}

/// An `Extrude.sheet_cut` cuts a sheet-metal body, and each sheet-metal body
/// has at most one governing rule (§6.6), in each reading where the features
/// are in force.
pub(super) fn check(index: &DesignIndex<'_>, findings: &mut Vec<Finding>) {
    for feature in &index.records.ir.design.features {
        let id = feature.id.as_str();
        let uncut = readings(index, MemberKey::Feature(id))
            .into_iter()
            .any(|reading| {
                reading.suppressed(index, feature) == Some(false)
                    && sheet_cut(index.records.definition_in(
                        feature,
                        reading.configuration,
                        reading.arrangement,
                    ))
                    && entering(index, id, reading).is_some_and(|state| !state.sheet_metal)
            });
        if uncut {
            super::error(
                findings,
                id,
                crate::rule!(
                    "design.sheet-cut-no-sheet-metal-body",
                    "Extrude.sheet_cut occurs in an extrude whose entering state holds no sheet-metal body"
                ),
            );
        }
    }
    for (&id, &container) in &index.records.containers {
        if let Container::Unordered { members, .. } = container {
            check_applied_rules(index, id, members, findings);
        }
    }
}

/// Two `ApplyRule` members of one unordered container that are in force in
/// one reading and pick one body name two rules.
fn check_applied_rules<'a>(
    index: &DesignIndex<'a>,
    container: &'a str,
    members: &'a BTreeSet<Member>,
    findings: &mut Vec<Finding>,
) {
    let features: Vec<&'a Feature> = members
        .iter()
        .filter_map(|member| match member {
            Member::Feature(id) => index.records.features.get(id.as_str()).copied(),
            Member::Container(_) => None,
        })
        .collect();
    let mut governed = BTreeSet::new();
    for reading in readings(index, MemberKey::Container(container)) {
        let mut applied: Vec<(&str, &SheetMetalRuleId, &SetSelection)> = Vec::new();
        for &feature in &features {
            if reading.suppressed(index, feature) != Some(false) {
                continue;
            }
            if let FeatureOperation::SheetMetal(sheet) = &index
                .records
                .definition_in(feature, reading.configuration, reading.arrangement)
                .operation
            {
                if let SheetMetalKind::ApplyRule {
                    rule,
                    bodies: Some(bodies),
                } = &sheet.kind
                {
                    applied.push((feature.id.as_str(), rule, bodies));
                }
            }
        }
        for (at, (first, rule, bodies)) in applied.iter().enumerate() {
            for (second, other, picked) in &applied[at + 1..] {
                if rule != other && bodies.at == picked.at && share_pick(bodies, picked) {
                    governed.insert((*first, *second, rule.as_str(), other.as_str()));
                }
            }
        }
    }
    for (first, second, rule, other) in governed {
        super::error(
            findings,
            second,
            crate::rule!(
                "design.apply-rules-govern-one-body",
                "ApplyRule features {first} and {second} of one unordered container make rules {rule} and {other} govern one body"
            ),
        );
    }
}

#[cfg(test)]
mod tests;
