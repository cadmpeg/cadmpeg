// SPDX-License-Identifier: Apache-2.0
//! Part 10, Products (§8.1): what an `External` definition holds, the
//! representations occurrences and representations name, instance-body
//! paths, generated members, and the placements an occurrence takes.

use crate::report::check::Ruled;
use std::collections::BTreeSet;

use crate::design::feature::{FeatureOperation, StoredOrigin};
use crate::design::refs::ConfigurationRef;
use crate::design::selection::{OccurrencePath, StepRef};
use crate::design::units::{Quantity, UnitSystem};
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::products::{
    MemberAction, ProductDefinition, Representation, RepresentationContent, Storage,
};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::transform::Transform;
use crate::validate::design::records::{DesignRecords, MemberKey};
use crate::validate::design::selections::{check_path, check_unconfigured};
use crate::validate::record_finding;

pub(super) fn check(records: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let ir = records.ir;
    let moved = records.moved_occurrences();
    for definition in &ir.model.product_definitions {
        let id = definition.id.as_str();
        if matches!(definition.storage, Storage::External(_))
            && (!definition.bodies.is_empty()
                || !definition.occurrences.is_empty()
                || records.scopes.contains_key(id))
        {
            error(
                findings,
                Check::ReferentialIntegrity,
                id,
                crate::rule!(
                    "products.external-definition-holds-bodies-occurrences-scope",
                    "an External definition holds bodies, occurrences, or a scope"
                ),
            );
        }
        for entry in &definition.instance_bodies {
            check_path(records, Some(id), &entry.occurrence, moved, id, findings);
            check_unconfigured(&entry.occurrence, id, "an instance-body path", findings);
        }
        if let Some(units) = &definition.units {
            check_units(id, units, findings);
        }
        check_representations(ir, records, moved, definition, findings);
        if let Some(generated) = &definition.generated_from {
            if let Some(factory) = &generated.definition {
                check_generated_from(records, id, factory, &generated.configuration, findings);
            }
        }
    }
    for occurrence in &ir.model.occurrences {
        let id = occurrence.id.as_str();
        if let Some(name) = &occurrence.representation {
            check_representation_name(
                records,
                Some(occurrence.prototype.as_str()),
                name.as_str(),
                id,
                findings,
            );
        }
        for nested in &occurrence.overrides {
            check_path(
                records,
                Some(occurrence.prototype.as_str()),
                &nested.path,
                moved,
                id,
                findings,
            );
            check_unconfigured(&nested.path, id, "a nested override's path", findings);
            if let Some(name) = &nested.representation {
                check_representation_name(
                    records,
                    last_prototype(records, &nested.path),
                    name.as_str(),
                    id,
                    findings,
                );
            }
        }
    }
    check_placements(ir, records, findings);
}

/// Each unit of a definition's authoring unit system is a `UnitSymbol` of
/// its quantity: a unit named outside an expression is one of the matching
/// quantity (§5.12).
fn check_units(id: &str, units: &UnitSystem, findings: &mut Vec<Finding>) {
    let mass = Quantity::of(0, 1, 0, 0);
    let time = Quantity::of(0, 0, 0, 1);
    let named = [
        ("length", Some(units.length), Quantity::LENGTH),
        ("angle", Some(units.angle), Quantity::ANGLE),
        ("mass", units.mass, mass),
        ("time", units.time, time),
    ];
    for (what, unit, quantity) in named {
        let Some(unit) = unit else { continue };
        if unit.canonical().1 != quantity {
            error(
                findings,
                Check::ParameterDomain,
                id,
                crate::rule!(
                    "products.definition-unit-system-not-quantity",
                    "the definition's {what} unit `{}` is no {what} unit",
                    unit.name()
                ),
            );
        }
    }
}

fn error(findings: &mut Vec<Finding>, check: Check, entity: &str, message: Ruled<'_>) {
    record_finding(findings, check, Severity::Error, Some(entity), message);
}

/// The prototype of a path's last occurrence, where the path ends in an
/// occurrence of the document.
pub(in crate::validate) fn last_prototype<'a>(
    records: &DesignRecords<'a>,
    path: &OccurrencePath,
) -> Option<&'a str> {
    match &path.0.last()?.occurrence {
        StepRef::Occurrence(occurrence) => records.prototype(occurrence.as_str()),
        StepRef::Native(_) => None,
    }
}

/// A representation name names a representation of `prototype`. Nothing is
/// reported where the prototype is unknown: the rules on prototypes and
/// path steps report that.
pub(in crate::validate) fn check_representation_name(
    records: &DesignRecords<'_>,
    prototype: Option<&str>,
    name: &str,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let Some(definition) = prototype.and_then(|prototype| records.definitions.get(prototype))
    else {
        return;
    };
    if !definition
        .representations
        .iter()
        .any(|representation| representation.name.as_str() == name)
    {
        error(
            findings,
            Check::ReferentialIntegrity,
            at,
            crate::rule!(
                "products.representation-no-representation",
                "representation `{name}` is no representation of {}",
                definition.id.as_str()
            ),
        );
    }
}

fn check_representations(
    ir: &CadIr,
    records: &DesignRecords<'_>,
    moved: &BTreeSet<&str>,
    definition: &ProductDefinition,
    findings: &mut Vec<Finding>,
) {
    let id = definition.id.as_str();
    let defaults = definition
        .representations
        .iter()
        .filter(|representation| representation.default == Some(true))
        .count();
    if defaults > 1 {
        error(
            findings,
            Check::Counts,
            id,
            crate::rule!(
                "products.representations-definition-default",
                "{defaults} representations of the definition are its default"
            ),
        );
    }
    for representation in &definition.representations {
        let name = representation.name.as_str();
        match &representation.content {
            RepresentationContent::Bodies(bodies) => {
                for body in bodies {
                    if !definition.bodies.contains(body) {
                        error(
                            findings,
                            Check::ReferentialIntegrity,
                            id,
                            crate::rule!("products.representation-names-no-body-definition", 
                                "representation `{name}` names {}, which is no body of the definition",
                                body.as_str()
                            ),
                        );
                    }
                }
            }
            RepresentationContent::Members(rule) => {
                if matches!(
                    rule.unlisted,
                    Some(MemberAction::Substitute(_) | MemberAction::ReplaceBy(_))
                ) {
                    error(
                        findings,
                        Check::ReferentialIntegrity,
                        id,
                        crate::rule!("products.representation-applies-substitution-instances-no-member", 
                            "representation `{name}` applies a substitution to the instances no member names"
                        ),
                    );
                }
                for member in &rule.members {
                    check_path(records, Some(id), &member.path, moved, id, findings);
                    check_unconfigured(
                        &member.path,
                        id,
                        "a representation member's path",
                        findings,
                    );
                    match &member.action {
                        MemberAction::Substitute(substitute) => check_representation_name(
                            records,
                            last_prototype(records, &member.path),
                            substitute.as_str(),
                            id,
                            findings,
                        ),
                        MemberAction::ReplaceBy(paths) => {
                            if paths.is_empty() {
                                error(
                                    findings,
                                    Check::ReferentialIntegrity,
                                    id,
                                    crate::rule!("products.representation-replaces-member-no-occurrence", 
                                        "representation `{name}` replaces a member by no occurrence"
                                    ),
                                );
                            }
                            for path in paths {
                                check_path(records, Some(id), path, moved, id, findings);
                                if rule.members.iter().any(|other| &other.path == path) {
                                    error(
                                        findings,
                                        Check::ReferentialIntegrity,
                                        id,
                                        crate::rule!("products.representation-replaces-member-member-own-path", 
                                            "representation `{name}` replaces a member by a member's own path"
                                        ),
                                    );
                                }
                            }
                        }
                        MemberAction::Include
                        | MemberAction::Exclude
                        | MemberAction::Detail(_)
                        | MemberAction::Native(_) => {}
                    }
                }
            }
            RepresentationContent::Document(_) | RepresentationContent::Unread => {}
        }
        check_representation_features(ir, records, id, representation, findings);
    }
}

/// Each `Representation.features` entry names a feature of the definition's
/// scope. With `design` out of scope the list is empty, which the scope rule
/// reports (§2.1). Which features are datum and curve features is not
/// checked: the specification does not say whether a sketch, a stored curve,
/// or an extracted curve is one.
fn check_representation_features(
    ir: &CadIr,
    records: &DesignRecords<'_>,
    definition: &str,
    representation: &Representation,
    findings: &mut Vec<Finding>,
) {
    if !ir.scope().holds(cadmpeg_core::decode::ArenaClass::Design) {
        return;
    }
    for feature in &representation.features {
        let feature = feature.as_str();
        // Until the v10 arenas go, a feature may still be a model feature.
        if ir
            .model
            .features
            .iter()
            .any(|row| row.id.as_str() == feature)
        {
            continue;
        }
        let scope = records
            .features
            .contains_key(feature)
            .then(|| records.scope_of(MemberKey::Feature(feature)))
            .flatten();
        let in_scope = scope.is_some_and(|scope| {
            scope == definition || records.spanned_definitions(scope).contains(definition)
        });
        if !in_scope {
            error(
                findings,
                Check::ReferentialIntegrity,
                definition,
                crate::rule!("products.representation-names-no-feature-definition-scope", 
                    "representation `{}` names {feature}, which is no feature of the definition's scope",
                    representation.name.as_str()
                ),
            );
        }
    }
}

/// A `GeneratedFrom.definition` is not the definition that holds it, and
/// where that definition holds configurations, `configuration` resolves
/// among them.
fn check_generated_from(
    records: &DesignRecords<'_>,
    holder: &str,
    factory: &ProductDefinitionId,
    configuration: &ConfigurationRef,
    findings: &mut Vec<Finding>,
) {
    let factory = factory.as_str();
    if factory == holder {
        error(
            findings,
            Check::ReferentialIntegrity,
            holder,
            crate::rule!(
                "products.definition-generated-itself",
                "the definition is generated from itself"
            ),
        );
    }
    let Some(configurations) = records.scope_configurations.get(factory) else {
        return;
    };
    let resolves = match configuration {
        ConfigurationRef::Local(local) => configurations
            .iter()
            .any(|candidate| candidate.id == *local),
        ConfigurationRef::Named(name) => configurations
            .iter()
            .any(|candidate| candidate.name == name.as_str()),
    };
    if !resolves {
        error(
            findings,
            Check::ReferentialIntegrity,
            holder,
            crate::rule!(
                "products.definition-generated-no-configuration",
                "the definition is generated from no configuration of {factory}"
            ),
        );
    }
}

/// Every placement of an occurrence and every `Link.transform` has a
/// nonsingular linear part (Warning: a source may store one).
fn check_placements(ir: &CadIr, records: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let mut placement = |holder: &str, transform: &Transform| {
        if transform.orientation().is_none() {
            record_finding(
                findings,
                Check::GeometricConsistency,
                Severity::Warning,
                Some(holder),
                crate::rule!(
                    "products.placement-linear-part-singular",
                    "a placement's linear part is singular"
                ),
            );
        }
    };
    for occurrence in &ir.model.occurrences {
        let id = occurrence.id.as_str();
        occurrence
            .transform
            .iter()
            .chain(
                occurrence
                    .overrides
                    .iter()
                    .flat_map(|nested| &nested.transform),
            )
            .for_each(|transform| placement(id, transform));
    }
    let design = &ir.design;
    let overrides = design
        .configurations
        .iter()
        .map(|configuration| (configuration.id.as_str(), &configuration.overrides))
        .chain(
            design
                .arrangements
                .iter()
                .map(|arrangement| (arrangement.id.as_str(), &arrangement.overrides)),
        );
    for (holder, overrides) in overrides {
        for value in overrides.occurrences.values() {
            value
                .transform
                .iter()
                .chain(value.nested.iter().flat_map(|nested| &nested.transform))
                .for_each(|transform| placement(holder, transform));
        }
    }
    for scope in &design.scopes {
        for current in &scope.current {
            for transform in current.poses.values() {
                placement(scope.definition.as_str(), transform);
            }
        }
    }
    for evaluation in &design.evaluations {
        if let Some(content) = &evaluation.content {
            for transform in content.delta.poses.values().flatten() {
                placement(evaluation.id.as_str(), transform);
            }
        }
    }
    for (holder, _, definition) in records.feature_definitions() {
        if let FeatureOperation::Stored {
            origin: StoredOrigin::Linked(link),
            ..
        } = &definition.operation
        {
            if let Some(transform) = &link.transform {
                placement(holder, transform);
            }
        }
    }
}

#[cfg(test)]
mod tests;
