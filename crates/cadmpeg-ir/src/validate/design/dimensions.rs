// SPDX-License-Identifier: Apache-2.0
//! Dimensions that bind design parameters (§5.12, §5.14).
//!
//! A bound design dimension is `{"kind": "parameter" | "negated", "value":
//! <id>}` on the wire, and the only other design value of that form is a
//! parameter reference's `target`, so a record's wire form names every
//! parameter its dimensions bind. Whether a bound parameter's kind matches
//! the dimension's value type is checked where the record is typed here: a
//! sketch constraint's dimensions.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value as Json;

use super::records::{DesignRecords, MemberKey};
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{RadialMeasure, SketchConstraint, SketchRelation, SketchSpace};
use crate::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use crate::design::units::{Quantity, ValueKind};
use crate::features::ParameterId;
use crate::report::check::Finding;
use crate::scalar::{Angle, Length, PositiveReal};

/// One parameter a dimension binds, with the record that holds it.
struct Binding<'a> {
    holder: &'a str,
    /// The scope of the dimension's owner, where it is known.
    scope: Option<&'a str>,
    parameter: String,
}

pub(super) fn check_dimensions(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let bindings = design_bindings(index);
    for binding in &bindings {
        match index.parameters.get(binding.parameter.as_str()) {
            None => super::error(
                findings,
                binding.holder,
                format_args!(
                    "a dimension binds {}, which is no parameter of the design",
                    binding.parameter
                ),
            ),
            Some(parameter) => {
                if let Some(scope) = binding.scope {
                    if parameter.scope.as_str() != scope {
                        super::error(
                            findings,
                            binding.holder,
                            format_args!(
                                "a dimension binds {}, a parameter of {}, not of its owner's scope {scope}",
                                binding.parameter,
                                parameter.scope.as_str()
                            ),
                        );
                    }
                }
            }
        }
    }
    let mut bound: BTreeSet<String> = bindings
        .into_iter()
        .map(|binding| binding.parameter)
        .collect();
    bound.extend(other_bindings(index));
    for parameter in &index.ir.design.parameters {
        if matches!(parameter.definition, ParameterDefinition::Driven)
            && !bound.contains(parameter.id.as_str())
        {
            super::warning(
                findings,
                parameter.id.as_str(),
                format_args!("driven parameter is bound by no dimension"),
            );
        }
    }
    let design = &index.ir.design;
    check_sketch_constraints(
        index,
        &design.sketch_constraints,
        |id| id.as_str(),
        findings,
    );
    check_sketch_constraints(
        index,
        &design.spatial_sketch_constraints,
        |id| id.as_str(),
        findings,
    );
}

/// The parameters bound by the dimensions of the design records whose owner
/// scope is known: features and the feature states of overrides, sketches
/// and their entities and constraints, containers, and sheet-metal rules.
fn design_bindings<'a>(index: &DesignRecords<'a>) -> Vec<Binding<'a>> {
    let design = &index.ir.design;
    let mut found = Vec::new();
    let mut add = |holder: &'a str, scope: Option<&'a str>, wire: Option<Json>| {
        for parameter in wire.iter().flat_map(bound_parameters) {
            found.push(Binding {
                holder,
                scope,
                parameter,
            });
        }
    };
    for feature in &design.features {
        let id = feature.id.as_str();
        add(id, index.scope_of(MemberKey::Feature(id)), wire_of(feature));
    }
    for configuration in &design.configurations {
        let scope = Some(configuration.scope.as_str());
        for state in configuration.overrides.features.values() {
            add(configuration.id.as_str(), scope, wire_of(state));
        }
    }
    for arrangement in &design.arrangements {
        let scope = Some(arrangement.scope.as_str());
        for state in arrangement.overrides.features.values() {
            add(arrangement.id.as_str(), scope, wire_of(state));
        }
    }
    for container in &design.containers {
        let id = container.id().as_str();
        add(
            id,
            index.scope_of(MemberKey::Container(id)),
            wire_of(container),
        );
    }
    let mut owners: BTreeMap<&str, Option<&'a str>> = BTreeMap::new();
    for sketch in &design.sketches {
        let scope = index.scope_of(MemberKey::Feature(sketch.owner.as_str()));
        owners.insert(sketch.id.as_str(), scope);
        add(sketch.id.as_str(), scope, wire_of(sketch));
    }
    for sketch in &design.spatial_sketches {
        let scope = index.scope_of(MemberKey::Feature(sketch.owner.as_str()));
        owners.insert(sketch.id.as_str(), scope);
        add(sketch.id.as_str(), scope, wire_of(sketch));
    }
    let owner = |sketch: &str| owners.get(sketch).copied().flatten();
    for entity in &design.sketch_entities {
        add(
            entity.id.as_str(),
            owner(entity.sketch.as_str()),
            wire_of(entity),
        );
    }
    for constraint in &design.sketch_constraints {
        add(
            constraint.id.as_str(),
            owner(constraint.sketch.as_str()),
            wire_of(constraint),
        );
    }
    for entity in &design.spatial_sketch_entities {
        add(
            entity.id.as_str(),
            owner(entity.sketch.as_str()),
            wire_of(entity),
        );
    }
    for constraint in &design.spatial_sketch_constraints {
        add(
            constraint.id.as_str(),
            owner(constraint.sketch.as_str()),
            wire_of(constraint),
        );
    }
    for rule in &design.sheet_metal_rules {
        add(rule.id.as_str(), Some(rule.scope.as_str()), wire_of(rule));
    }
    found
}

/// The design parameters a PMI annotation or a process record binds, which
/// count as dimensions that bind a driven parameter.
fn other_bindings(index: &DesignRecords<'_>) -> Vec<String> {
    let ir = index.ir;
    let mut found = Vec::new();
    for annotation in &ir.model.pmi {
        found.extend(
            wire_of(annotation)
                .map(|wire| bound_parameters(&wire))
                .into_iter()
                .flatten(),
        );
    }
    if let Ok(process) = serde_json::to_value(&ir.process) {
        found.extend(design_targets(&process));
    }
    found
}

fn wire_of<T: Serialize>(record: &T) -> Option<Json> {
    serde_json::to_value(record).ok()
}

/// The ids a wire value binds as `{"kind": "parameter" | "negated",
/// "value": <id>}` outside a parameter reference's `target`.
fn bound_parameters(wire: &Json) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![(wire, false)];
    while let Some((node, in_target)) = pending.pop() {
        match node {
            Json::Array(items) => pending.extend(items.iter().map(|item| (item, false))),
            Json::Object(object) => {
                let binding = object.len() == 2
                    && matches!(
                        object.get("kind").and_then(Json::as_str),
                        Some("parameter" | "negated")
                    );
                if let (true, false, Some(Json::String(id))) =
                    (binding, in_target, object.get("value"))
                {
                    found.push(id.clone());
                    continue;
                }
                pending.extend(object.iter().map(|(key, item)| (item, key == "target")));
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    found
}

/// The design parameter ids a wire value names as a parameter reference's
/// `target`.
fn design_targets(wire: &Json) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![wire];
    while let Some(node) = pending.pop() {
        match node {
            Json::Array(items) => pending.extend(items),
            Json::Object(object) => {
                if let Some(Json::Object(target)) = object.get("target") {
                    if target.get("kind").and_then(Json::as_str) == Some("parameter") {
                        if let Some(Json::String(id)) = target.get("value") {
                            found.push(id.clone());
                        }
                    }
                }
                pending.extend(object.values());
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    found
}

/// The value type of a dimension: the kind a parameter it binds has, whether
/// `Negated` applies to it, and the values it admits.
trait Bound {
    const NAME: &'static str;
    const SIGNED: bool;
    fn kind() -> ValueKind;
    fn admits(value: &ParameterValue) -> bool;
}

impl Bound for Length {
    const NAME: &'static str = "length";
    const SIGNED: bool = true;
    fn kind() -> ValueKind {
        ValueKind::Quantity(Quantity::LENGTH)
    }
    fn admits(value: &ParameterValue) -> bool {
        matches!(value, ParameterValue::Number(_))
    }
}

impl Bound for Angle {
    const NAME: &'static str = "angle";
    const SIGNED: bool = true;
    fn kind() -> ValueKind {
        ValueKind::Quantity(Quantity::ANGLE)
    }
    fn admits(value: &ParameterValue) -> bool {
        matches!(value, ParameterValue::Number(_))
    }
}

impl Bound for PositiveReal {
    const NAME: &'static str = "positive real";
    const SIGNED: bool = false;
    fn kind() -> ValueKind {
        ValueKind::Quantity(Quantity::REAL)
    }
    fn admits(value: &ParameterValue) -> bool {
        matches!(value, ParameterValue::Number(number) if number.get() > 0.0)
    }
}

impl Bound for bool {
    const NAME: &'static str = "Boolean";
    const SIGNED: bool = true;
    fn kind() -> ValueKind {
        ValueKind::Boolean
    }
    fn admits(value: &ParameterValue) -> bool {
        matches!(value, ParameterValue::Boolean(_))
    }
}

/// The parameter a dimension binds, and the record when it exists.
fn bound<'a, T>(
    index: &DesignRecords<'a>,
    dimension: &'a Dimension<T>,
) -> Option<(&'a ParameterId, bool, Option<&'a DesignParameter>)> {
    let (parameter, negated) = match dimension {
        Dimension::Literal(_) => return None,
        Dimension::Parameter(parameter) => (parameter, false),
        Dimension::Negated(parameter) => (parameter, true),
    };
    Some((
        parameter,
        negated,
        index.parameters.get(parameter.as_str()).copied(),
    ))
}

/// A bound dimension's parameter has the kind its value type takes, a cached
/// value it admits (Warning), and `Negated` binds only a signed quantity or
/// a Boolean.
fn check_bound<T: Bound>(
    index: &DesignRecords<'_>,
    holder: &str,
    dimension: &Dimension<T>,
    findings: &mut Vec<Finding>,
) {
    let Some((id, negated, record)) = bound(index, dimension) else {
        return;
    };
    let id = id.as_str();
    if negated && !T::SIGNED {
        super::error(
            findings,
            holder,
            format_args!(
                "a {} dimension is `negated`; only a signed quantity or a Boolean is",
                T::NAME
            ),
        );
    }
    let Some(record) = record else {
        return;
    };
    if record.kind != T::kind() {
        super::error(
            findings,
            holder,
            format_args!(
                "a {} dimension binds {id}, whose kind is {:?}",
                T::NAME,
                record.kind
            ),
        );
        return;
    }
    if let Some(value) = &record.value {
        if !T::admits(value) {
            super::warning(
                findings,
                holder,
                format_args!(
                    "a {} dimension binds {id}, whose cached value {value:?} it does not admit",
                    T::NAME
                ),
            );
        }
    }
}

fn check_sketch_constraints<S: SketchSpace>(
    index: &DesignRecords<'_>,
    constraints: &[SketchConstraint<S>],
    id: fn(&S::Constraint) -> &str,
    findings: &mut Vec<Finding>,
) {
    for constraint in constraints {
        let holder = id(&constraint.id);
        if let Some(active) = &constraint.active {
            check_bound(index, holder, active, findings);
        }
        match &constraint.relation {
            SketchRelation::Distance { value, .. } | SketchRelation::Length { value, .. } => {
                check_bound(index, holder, value, findings);
            }
            SketchRelation::Angle { value, .. } => check_bound(index, holder, value, findings),
            SketchRelation::Radial { measure, value, .. } => {
                check_bound(index, holder, value, findings);
                let diameter = matches!(measure, RadialMeasure::Diameter);
                if let (true, Some((parameter, _, Some(record)))) = (diameter, bound(index, value))
                {
                    if record.display.is_some() {
                        super::error(
                            findings,
                            holder,
                            format_args!(
                                "a diameter dimension binds {}, which states a `display`",
                                parameter.as_str()
                            ),
                        );
                    }
                }
            }
            SketchRelation::Weight { value, .. } => check_bound(index, holder, value, findings),
            SketchRelation::Coincident { .. }
            | SketchRelation::Concentric { .. }
            | SketchRelation::Collinear { .. }
            | SketchRelation::Parallel { .. }
            | SketchRelation::Perpendicular { .. }
            | SketchRelation::Tangent { .. }
            | SketchRelation::Curvature { .. }
            | SketchRelation::Horizontal { .. }
            | SketchRelation::Vertical { .. }
            | SketchRelation::Midpoint { .. }
            | SketchRelation::Symmetric { .. }
            | SketchRelation::Equal { .. }
            | SketchRelation::EqualDistance { .. }
            | SketchRelation::Fixed { .. }
            | SketchRelation::Offset(_)
            | SketchRelation::Pattern(_)
            | SketchRelation::Polygon(_)
            | SketchRelation::Slot(_)
            | SketchRelation::Pierce { .. }
            | SketchRelation::FixedLength { .. }
            | SketchRelation::FixedAngle { .. }
            | SketchRelation::Native { .. } => {}
        }
    }
}
