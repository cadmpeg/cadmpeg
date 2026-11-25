// SPDX-License-Identifier: Apache-2.0
//! The sheet-metal bodies of a state and the rules that govern them (§6.6):
//! whether a sheet-metal feature precedes a feature, the literal thickness
//! of each sheet where the features before it keep every sheet's thickness
//! edges, an `Extrude.sheet_cut` cuts a sheet-metal body, and each body has
//! at most one governing rule.

use crate::design::container::Container;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ids::SheetMetalRuleId;
use crate::design::ops::sheet_metal::{Flange, Hem, SheetMetalKind};
use crate::design::options::{Boolean, BooleanOp, Thickness};
use crate::design::refs::Member;
use crate::design::selection::SetSelection;
use crate::report::check::Finding;

use super::index::{DesignIndex, StateAt};
use super::ops::picks::share_pick;
use super::picked::literal;

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

/// The sheet-metal bodies of the state entering a feature.
pub(super) struct Entering {
    /// Whether a sheet-metal feature makes or changes a body of the state.
    pub(super) sheet_metal: bool,
    /// The literal thicknesses the sheet-metal features state, where every
    /// feature of the state keeps each sheet's thickness edges whole.
    pub(super) thicknesses: Option<Vec<f64>>,
}

/// What an operation does to the sheet-metal bodies of a state.
enum Effect {
    /// It changes no body.
    Keeps,
    /// It can reshape a body and makes no sheet-metal body.
    Reshapes,
    /// It is a sheet-metal feature, with the literal thicknesses it states.
    Sheet(Option<Vec<f64>>),
    /// Its effect is not read.
    Unknown,
}

/// The sheet-metal bodies entering `feature`, a member of an ordered
/// container. `None` where a feature of the state has an effect that is not
/// read, or the definition takes bodies from elsewhere.
pub(super) fn entering<'a>(index: &DesignIndex<'a>, feature: &'a str) -> Option<Entering> {
    index.entering(feature)?;
    let owner = index.owner_definition(feature)?;
    let definition = index.records.definitions.get(owner.as_str())?;
    if definition.generated_from.is_some()
        || !definition.occurrences.is_empty()
        || !definition.instance_bodies.is_empty()
    {
        return None;
    }
    let mut entering = Entering {
        sheet_metal: false,
        thicknesses: Some(Vec::new()),
    };
    for id in index.state_features(StateAt::Entering(feature))?.iter() {
        let record = index.records.features.get(id)?;
        for definition in definitions(index, record) {
            match effect(index, &definition.operation) {
                Effect::Keeps => {}
                Effect::Reshapes => entering.thicknesses = None,
                Effect::Sheet(found) => {
                    entering.sheet_metal = true;
                    match (&mut entering.thicknesses, found) {
                        (Some(all), Some(found)) => all.extend(found),
                        (all, _) => *all = None,
                    }
                }
                Effect::Unknown => return None,
            }
        }
    }
    Some(entering)
}

/// The definitions of a feature: its own and each override's.
fn definitions<'i, 'a>(
    index: &'i DesignIndex<'a>,
    feature: &'a Feature,
) -> impl Iterator<Item = &'a FeatureDefinition> + 'i {
    let records = &index.records;
    let configurations = records
        .configurations
        .values()
        .copied()
        .map(|c| &c.overrides);
    let arrangements = records.arrangements.values().copied().map(|a| &a.overrides);
    let overrides = configurations
        .chain(arrangements)
        .filter_map(|overrides| overrides.features.get(&feature.id)?.definition.as_ref());
    std::iter::once(&feature.definition).chain(overrides)
}

fn effect(index: &DesignIndex<'_>, operation: &FeatureOperation) -> Effect {
    match operation {
        FeatureOperation::Sketch(_)
        | FeatureOperation::SpatialSketch(_)
        | FeatureOperation::Spreadsheet(_)
        | FeatureOperation::Datum(_)
        | FeatureOperation::Curve(_) => Effect::Keeps,
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
        | FeatureOperation::DeleteBodies(_)
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

/// An `Extrude.sheet_cut` cuts a sheet-metal body, and each sheet-metal body
/// has at most one governing rule (§6.6).
pub(super) fn check(index: &DesignIndex<'_>, findings: &mut Vec<Finding>) {
    for feature in &index.records.ir.design.features {
        let sheet_cut = definitions(index, feature).any(|definition| {
            matches!(
                &definition.operation,
                FeatureOperation::Extrude(extrude)
                    if extrude.sheet_cut.is_some()
                        && matches!(
                            extrude.output.boolean,
                            Some(Boolean::Apply { op: BooleanOp::Cut, .. })
                        )
            )
        });
        if sheet_cut && entering(index, feature.id.as_str()).is_some_and(|state| !state.sheet_metal)
        {
            super::error(
                findings,
                feature.id.as_str(),
                crate::rule!(
                    "design.sheet-cut-no-sheet-metal-body",
                    "Extrude.sheet_cut occurs in an extrude that no sheet-metal feature precedes"
                ),
            );
        }
    }
    for container in index.records.containers.values() {
        if let Container::Unordered { members, .. } = container {
            check_applied_rules(index, members.iter(), findings);
        }
    }
}

/// Two `ApplyRule` members of one unordered container that pick one body
/// name one rule.
fn check_applied_rules<'a>(
    index: &DesignIndex<'a>,
    members: impl Iterator<Item = &'a Member>,
    findings: &mut Vec<Finding>,
) {
    let mut applied: Vec<(&str, &SheetMetalRuleId, &SetSelection)> = Vec::new();
    for member in members {
        let Member::Feature(id) = member else {
            continue;
        };
        let Some(feature) = index.records.features.get(id.as_str()) else {
            continue;
        };
        for definition in definitions(index, feature) {
            if let FeatureOperation::SheetMetal(sheet) = &definition.operation {
                if let SheetMetalKind::ApplyRule {
                    rule,
                    bodies: Some(bodies),
                } = &sheet.kind
                {
                    applied.push((feature.id.as_str(), rule, bodies));
                }
            }
        }
    }
    for (at, (first, rule, bodies)) in applied.iter().enumerate() {
        for (second, other, picked) in applied.iter().skip(at + 1) {
            if first != second
                && rule != other
                && bodies.at == picked.at
                && share_pick(bodies, picked)
            {
                super::error(
                    findings,
                    second,
                    crate::rule!(
                        "design.apply-rules-govern-one-body",
                        "ApplyRule features {first} and {second} of one unordered container make rules {} and {} govern one body",
                        rule.as_str(),
                        other.as_str()
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
