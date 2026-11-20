// SPDX-License-Identifier: Apache-2.0
//! The geometry-true form of a design scope (cadir-v11.md §3.4, Features
//! without their state): the scope's parameters, and one stored-geometry
//! feature of the current bodies.
//!
//! It runs where no native Design record is written and the document holds a
//! design scope for its root definition. A failure here is a
//! [`super::reduce::Stage::NeutralDesign`] failure. Its fallbacks write the
//! design without its `Sketch` items, then write no design: they never drop
//! a body.

mod base_feature;
mod expression;
mod extrude;
pub(super) mod frames;
pub(super) mod history;
pub(super) mod parameters;
mod roster;
mod sketch;
#[cfg(test)]
mod tests;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use cadmpeg_ir::codec::write::evaluation_missing;
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::parameter::ParameterOwner;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::report::loss::LossNote;

use super::presentation::GeneratedDesignRegistry;
use crate::native::F3dNative;
use base_feature::BaseFeaturePlan;
use extrude::Decision;
use frames::DesignRecords;
use parameters::ParameterPlan;
use sketch::SketchPlan;

#[cfg(test)]
thread_local! {
    /// Makes the next attempt that writes the design scope on this thread
    /// fail in the design module.
    static FAIL_NEXT_PLAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
thread_local! {
    /// Makes the next attempt that writes a `Sketch` item on this thread
    /// fail in the sketch records.
    static FAIL_NEXT_SKETCHES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The failure [`FAIL_NEXT_SKETCHES`] requests, once.
#[cfg(test)]
fn requested_sketch_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_SKETCHES
        .with(|fail| fail.replace(false))
        .then(|| {
            cadmpeg_core::CodecError::NotImplemented("a test failure of the sketch records".into())
        })
}

/// The failure [`FAIL_NEXT_PLAN`] requests, once.
#[cfg(test)]
pub(super) fn requested_test_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_PLAN.with(|fail| fail.replace(false)).then(|| {
        cadmpeg_core::CodecError::NotImplemented("a test failure of the design module".into())
    })
}

/// What the writer writes of the root definition's design scope.
pub(super) struct NeutralDesign<'a> {
    parameters: ParameterPlan,
    sketches: SketchPlan<'a>,
    /// The scope's extrude, written as an `Extrude` item in place of the
    /// Base Feature where the rule of [`extrude::plan`] allows it.
    extrude: Decision<'a>,
    base_feature: BaseFeaturePlan<'a>,
    losses: Vec<LossNote>,
}

/// The plan for the root definition's design scope; `None` when the
/// document holds none. With `sketch_failure`, the reason a design with
/// its sketches failed, the plan writes no `Sketch` item. `extrude_failure`
/// is the error of an earlier attempt that wrote the scope's extrude as an
/// `Extrude` item; the plan then writes it in the Base Feature.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    sketch_failure: Option<&str>,
    extrude_failure: Option<&str>,
) -> Result<Option<NeutralDesign<'a>>, CodecError> {
    let Some(scope) = root_scope(target) else {
        return Ok(None);
    };
    let mut parameters = parameters::plan(target, &scope.definition);
    let features = scope_features(target, scope);
    let mut sketches = sketch_plan(target, &features);
    if let Some(reason) = sketch_failure {
        sketches.drop_items(reason);
    }
    let extrude = if target.geometry.bodies.is_empty() {
        Decision::None
    } else {
        extrude::plan(
            target,
            scope,
            &features,
            &sketches,
            &mut parameters,
            extrude_failure,
        )
    };
    let written = parameters.written().collect::<BTreeSet<_>>();
    let mut owned = BTreeMap::<&FeatureId, Vec<String>>::new();
    for parameter in &target.design.parameters {
        if let Some(ParameterOwner::Feature(feature)) = &parameter.owner {
            if written.contains(&parameter.id) {
                owned
                    .entry(feature)
                    .or_default()
                    .push(parameter.id.to_string());
            }
        }
    }
    let unwritten = features
        .iter()
        .filter(|feature| !sketches.writes(feature) && !extrude.writes(feature))
        .map(|feature| {
            let mut notes = Vec::new();
            if let Some(reason) = sketches.unwritten.get(*feature) {
                notes.push(format!("its sketch {reason}"));
            }
            if let Some(reason) = extrude.reason(feature) {
                notes.push(format!("the Base Feature holds it: {reason}"));
            }
            if let Some(parameters) = owned.get(*feature) {
                notes.push(format!(
                    "its parameters are written as user parameters: {}",
                    parameters.join(" ")
                ));
            }
            if notes.is_empty() {
                feature.to_string()
            } else {
                format!("{feature} ({})", notes.join("; "))
            }
        });
    let reason = if matches!(extrude, Decision::Extrude(_)) {
        "stores no timeline item for these features of the design"
    } else {
        "stores the current bodies as one Base Feature and no timeline item for these features of the design"
    };
    let losses = evaluation_missing("F3D", reason, unwritten)
        .into_iter()
        .collect();
    let base_feature = base_feature::plan(target, sketches.items.len())?;
    Ok(Some(NeutralDesign {
        parameters,
        sketches,
        extrude,
        base_feature,
        losses,
    }))
}

impl NeutralDesign<'_> {
    /// Whether the design stores a body: its B-rep stream then carries the
    /// Base Feature's names and history.
    pub(super) fn has_bodies(&self) -> bool {
        self.base_feature.has_item()
    }

    /// Name the result bodies and their faces in `native`, whose records the
    /// B-rep stream is written from.
    pub(super) fn stamp(&mut self, native: &mut F3dNative) -> Result<(), CodecError> {
        match &self.extrude {
            Decision::Extrude(plan) => extrude::stamp(plan, &self.sketches.items, native),
            Decision::None | Decision::BaseFeature { .. } => {
                let losses = self.base_feature.stamp(native)?;
                self.losses.extend(losses);
                Ok(())
            }
        }
    }

    /// The parameter records and the design's records, with their types
    /// registered in `registry`.
    pub(super) fn records(
        &mut self,
        registry: &mut GeneratedDesignRegistry,
    ) -> Result<(Vec<u8>, DesignRecords), CodecError> {
        let (parameter_bytes, records, losses) =
            self.base_feature.encode(&self.parameters, registry)?;
        self.losses.extend(losses);
        Ok((parameter_bytes, records))
    }

    /// The members of the design folder of a design with result bodies:
    /// the Design, ACT and Browser segments, the history stream `smbh`, the
    /// snapshot stream `smb` and the members their records name (f3d.md §3.1, "Records of a Base
    /// Feature design").
    pub(super) fn archive(
        &mut self,
        target: &CadIr,
        smbh: &[u8],
        smb: &[u8],
    ) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
        let scope = root_scope(target)
            .ok_or_else(|| CodecError::Malformed("the design has no root scope".into()))?;
        let bodies = self.base_feature.roster_bodies(target);
        let extrude = match &self.extrude {
            Decision::Extrude(plan) => Some(plan.as_ref()),
            Decision::None | Decision::BaseFeature { .. } => None,
        };
        let roster = roster::Roster::new(
            bodies.len(),
            self.parameters.written_count(),
            &self.sketches.items,
            extrude,
        )?;
        let ctx = roster::Context {
            table: roster::type_table(&roster, bodies.iter().any(|body| body.sheet))?,
            roster,
            values: roster::values::Values::new(target, &scope.definition, smbh),
            bodies,
            sketches: &self.sketches.items,
            extrude,
        };
        let mut records = DesignRecords::default();
        BaseFeaturePlan::roster_records(&ctx, &self.parameters, &mut records)?;
        let (members, losses) = roster::archive(&ctx, records, smbh, smb)?;
        self.losses.extend(losses);
        Ok(members)
    }

    /// Whether the design writes a `Sketch` item.
    pub(super) fn writes_sketches(&self) -> bool {
        !self.sketches.items.is_empty()
    }

    pub(super) fn into_losses(self) -> Vec<LossNote> {
        let mut losses = self.losses;
        losses.extend(self.parameters.into_losses());
        losses.extend(self.sketches.into_losses());
        losses
    }
}

/// Whether the root definition's design scope writes a `Sketch` item.
pub(super) fn writes_sketches(target: &CadIr) -> bool {
    root_scope(target).is_some_and(|scope| {
        !sketch_plan(target, &scope_features(target, scope))
            .items
            .is_empty()
    })
}

/// The plan of the sketches among `features`. The `Sketch` items precede
/// the `Base Feature`, which a design with no body does not write.
fn sketch_plan<'a>(target: &'a CadIr, features: &[&'a FeatureId]) -> SketchPlan<'a> {
    if target.geometry.bodies.is_empty() {
        SketchPlan::default()
    } else {
        sketch::plan(target, features)
    }
}

/// The design scope of the root definition: the document's one scope, or
/// the scope whose definition no occurrence places.
pub(super) fn root_scope(target: &CadIr) -> Option<&DesignScope> {
    match target.design.scopes.as_slice() {
        [] => None,
        [scope] => Some(scope),
        scopes => {
            let placed = target
                .model
                .occurrences
                .iter()
                .map(|occurrence| &occurrence.prototype)
                .collect::<BTreeSet<_>>();
            scopes
                .iter()
                .find(|scope| !placed.contains(&scope.definition))
        }
    }
}

/// The ids of the root scope's design parameters: the neutral parameters
/// they restate are written or charged with them.
pub(super) fn restated_parameters(target: &CadIr) -> BTreeSet<&str> {
    root_scope(target).map_or_else(BTreeSet::new, |scope| {
        target
            .design
            .parameters
            .iter()
            .filter(|parameter| parameter.scope == scope.definition)
            .map(|parameter| parameter.id.as_str())
            .collect()
    })
}

/// The root scope's features and parameters, for the losses of a design the
/// writer drops.
pub(super) fn scope_records(target: &CadIr) -> (Vec<String>, Vec<String>) {
    let Some(scope) = root_scope(target) else {
        return (Vec::new(), Vec::new());
    };
    let features = scope_features(target, scope)
        .into_iter()
        .map(ToString::to_string)
        .collect();
    let parameters = target
        .design
        .parameters
        .iter()
        .filter(|parameter| parameter.scope == scope.definition)
        .map(|parameter| parameter.id.to_string())
        .collect();
    (features, parameters)
}

/// Every feature of the scope's containers, in container order.
fn scope_features<'a>(target: &'a CadIr, scope: &'a DesignScope) -> Vec<&'a FeatureId> {
    let containers = target
        .design
        .containers
        .iter()
        .map(|container| (container.id(), container))
        .collect::<BTreeMap<_, _>>();
    let mut features = Vec::new();
    let mut visited = BTreeSet::new();
    for root in &scope.roots {
        walk(root, &containers, &mut visited, &mut features);
    }
    features
}

fn walk<'a>(
    id: &'a ContainerId,
    containers: &BTreeMap<&'a ContainerId, &'a Container>,
    visited: &mut BTreeSet<&'a ContainerId>,
    features: &mut Vec<&'a FeatureId>,
) {
    if !visited.insert(id) {
        return;
    }
    let Some(container) = containers.get(id) else {
        return;
    };
    let members: Vec<&Member> = match container {
        Container::Ordered { members, .. } => members.iter().collect(),
        Container::Unordered { members, .. } => members.iter().collect(),
    };
    for member in members {
        match member {
            Member::Feature(feature) => features.push(feature),
            Member::Container(nested) => walk(nested, containers, visited, features),
        }
    }
}
