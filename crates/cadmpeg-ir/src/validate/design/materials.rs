// SPDX-License-Identifier: Apache-2.0
//! A configuration's `materials` (§5.11): a `target` picks bodies of the
//! scope's definitions, and no two entries select one body.
//!
//! An entry selects the bodies its picks' `resolved` entities name, and, in
//! the scope's active configuration, whose current states are stored, the
//! bodies its names match there. A configuration that is not active has no
//! stored current states, so only `resolved` decides there.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::names::NameKind;
use crate::design::refs::EntityRef;
use crate::design::selection::{PickTarget, Picked, Picks};
use crate::report::check::Finding;

use super::index::DesignIndex;
use super::matching::Matchers;
use super::names::derived_kinds;
use super::selections::{resolving_definition, Stored};

pub(super) fn check<'a>(
    index: &DesignIndex<'a>,
    matchers: &Matchers<'a>,
    findings: &mut Vec<Finding>,
) {
    let ir = index.records.ir;
    let owners: BTreeMap<&str, &str> = ir
        .model
        .product_definitions
        .iter()
        .flat_map(|definition| {
            definition
                .bodies
                .iter()
                .map(move |body| (body.as_str(), definition.id.as_str()))
        })
        .collect();
    let bodies: BTreeSet<&str> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    for configuration in &ir.design.configurations {
        let at = configuration.id.as_str();
        let scope = configuration.scope.as_str();
        let spanned = index.records.spanned_definitions(scope);
        let active = index
            .records
            .scopes
            .get(scope)
            .and_then(|record| record.active_configuration.as_ref())
            == Some(&configuration.id);
        let extents = matchers.states().extents_for_scope(scope);
        let matcher = (active && !extents.is_empty())
            .then(|| matchers.get(&Stored::Extents(extents)))
            .flatten();
        let mut selected: Vec<(usize, BTreeSet<&str>)> = Vec::new();
        for (entry, material) in configuration.materials.iter().enumerate() {
            let Some(target) = &material.target else {
                continue;
            };
            let Picks::Named(picks) = &target.picks else {
                continue;
            };
            let mut chosen = BTreeSet::new();
            for pick in picks.as_slice() {
                let target_is_body = picks_bodies(pick);
                if !target_is_body {
                    super::error(
                        findings,
                        at,
                        crate::rule!(
                            "design.material-target-picks-something-other-body",
                            "a material's target picks something other than a body"
                        ),
                    );
                }
                let definition = resolving_definition(index, Some(&configuration.scope), pick);
                if let Some(definition) = definition {
                    if !spanned.is_empty() && !spanned.contains(definition) {
                        super::error(findings, at, crate::rule!("design.material-target-picks-no-definition-scope", "a material's target picks in {definition}, which is no definition the scope spans"));
                    }
                }
                match &pick.resolved {
                    Some(resolved) => {
                        for entity in resolved {
                            let EntityRef::Body(body) = entity else {
                                if target_is_body {
                                    super::error(
                                        findings,
                                        at,
                                        crate::rule!(
                                            "design.material-target-resolves-no-body",
                                            "a material's target resolves to {}, which is no body",
                                            entity.id()
                                        ),
                                    );
                                }
                                continue;
                            };
                            let owner = owners.get(body.as_str());
                            if let Some(owner) = owner {
                                if !spanned.is_empty() && !spanned.contains(owner) {
                                    super::error(findings, at, crate::rule!("design.material-target-resolves-body-no-definition", "a material's target resolves to {}, a body of {owner}, which is no definition the scope spans", body.as_str()));
                                }
                            }
                            chosen.insert(body.as_str());
                        }
                    }
                    None => {
                        if let (Some(matcher), PickTarget::Entity(name), None) =
                            (&matcher, &pick.target, &pick.context)
                        {
                            chosen.extend(
                                matcher
                                    .matched(name)
                                    .into_iter()
                                    .filter(|entity| bodies.contains(entity)),
                            );
                        }
                    }
                }
            }
            for (earlier, bodies) in &selected {
                if let Some(body) = bodies.intersection(&chosen).next() {
                    super::error(findings, at, crate::rule!("design.materials-both-select-body-no-two", "materials {earlier} and {entry} both select body {body}; no two entries select one body"));
                }
            }
            selected.push((entry, chosen));
        }
    }
}

/// Whether a pick may pick bodies: a body name, a feature's result, or a
/// target whose kind is not stated.
fn picks_bodies(pick: &Picked) -> bool {
    match &pick.target {
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.is_empty() || kinds.contains(&NameKind::Body)
        }
        PickTarget::Native { .. }
        | PickTarget::FeatureResult(_)
        | PickTarget::Publication(_)
        | PickTarget::TemplateInput { .. } => true,
        PickTarget::Sketch(_)
        | PickTarget::SketchEntity(_)
        | PickTarget::SpatialSketchEntity(_)
        | PickTarget::SketchRegion { .. }
        | PickTarget::SketchFrame { .. }
        | PickTarget::Datum { .. }
        | PickTarget::Principal { .. }
        | PickTarget::Feature(_)
        | PickTarget::Component => false,
    }
}

#[cfg(test)]
mod tests;
