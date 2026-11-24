// SPDX-License-Identifier: Apache-2.0
//! Configurations and arrangements (§5.1, §5.11), and the tolerance and
//! decimals of design parameters (§5.12).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;

use super::records::DesignRecords;
use crate::design::configuration::{
    Arrangement, ArrangementKind, ArrangementRef, DefinitionOverride, FeatureStateOverride,
    OccurrenceOverride, Overrides, ParameterOverride, PartNumber,
};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::ops::body::TransformStep;
use crate::design::parameter::{
    DesignParameter, Deviations, FitDeviations, ParameterDefinition, Tolerance,
};
use crate::design::refs::ConfigurationRef;
use crate::design::scope::DesignScope;
use crate::design::selection::Anchor;
use crate::design::units::{Quantity, ValueKind};
use crate::ids::ProductDefinitionId;
use crate::products::Storage;
use crate::report::check::Finding;
use crate::validate::design::selections::{check_path, check_unconfigured, moved_occurrences};
use crate::validate::products::definitions::{check_representation_name, last_prototype};

/// What holds a set of overrides.
#[derive(Clone, Copy)]
enum Holder {
    /// A configuration, and whether it states a parent.
    Configuration { has_parent: bool },
    /// An arrangement.
    Arrangement,
}

pub(super) fn check_configurations(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &index.ir.design;
    for scope in &design.scopes {
        check_scope(index, scope, findings);
    }
    for configuration in &design.configurations {
        let id = configuration.id.as_str();
        let scope = configuration.scope.as_str();
        if !index.scopes.contains_key(scope) {
            super::error(
                findings,
                id,
                format_args!("configuration's scope {scope} has no design scope"),
            );
        }
        if let Some(parent) = &configuration.parent {
            let parent = parent.as_str();
            match index.configurations.get(parent) {
                None => super::error(
                    findings,
                    id,
                    format_args!(
                        "configuration's parent {parent} is no configuration of the design"
                    ),
                ),
                Some(parent) if parent.scope != configuration.scope => super::error(
                    findings,
                    id,
                    format_args!(
                        "configuration's parent {} is a configuration of another scope",
                        parent.id.as_str()
                    ),
                ),
                Some(_) => {}
            }
            if parent_cycle(index, id) {
                super::error(
                    findings,
                    id,
                    format_args!("configuration is its own ancestor through `parent`"),
                );
            }
        }
        check_overrides(
            index,
            id,
            scope,
            &configuration.overrides,
            Holder::Configuration {
                has_parent: configuration.parent.is_some(),
            },
            findings,
        );
        check_active_arrangements(
            index,
            id,
            scope,
            &configuration.active_arrangements,
            findings,
        );
        let untargeted = configuration
            .materials
            .iter()
            .filter(|material| material.target.is_none())
            .count();
        if untargeted > 1 {
            super::error(
                findings,
                id,
                format_args!("configuration states {untargeted} materials with no target; it states at most one"),
            );
        }
        for target in configuration
            .materials
            .iter()
            .filter_map(|material| material.target.as_ref())
        {
            if !matches!(target.at, Anchor::Current) {
                super::error(
                    findings,
                    id,
                    format_args!("a material's target is not anchored `current`"),
                );
            }
        }
    }
    for arrangement in &design.arrangements {
        check_arrangement(index, arrangement, findings);
    }
    check_arrangements_in_force(index, findings);
    for parameter in &design.parameters {
        check_parameter_tolerance(parameter, findings);
    }
}

fn parent_cycle(index: &DesignRecords<'_>, id: &str) -> bool {
    let mut next = index
        .configurations
        .get(id)
        .and_then(|configuration| configuration.parent.as_ref());
    for _ in 0..index.configurations.len() {
        let Some(parent) = next else {
            return false;
        };
        if parent.as_str() == id {
            return true;
        }
        next = index
            .configurations
            .get(parent.as_str())
            .and_then(|configuration| configuration.parent.as_ref());
    }
    false
}

fn check_scope(index: &DesignRecords<'_>, scope: &DesignScope, findings: &mut Vec<Finding>) {
    let id = scope.definition.as_str();
    match (&scope.active_configuration, index.has_configurations(id)) {
        (None, true) => super::error(
            findings,
            id,
            format_args!("scope has configurations but states no active configuration"),
        ),
        (Some(active), false) => super::error(
            findings,
            id,
            format_args!(
                "scope names active configuration {}, but it has no configurations",
                active.as_str()
            ),
        ),
        (Some(active), true) => {
            let of_scope = index
                .configurations
                .get(active.as_str())
                .is_some_and(|configuration| configuration.scope == scope.definition);
            if !of_scope {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "scope's active configuration {} is no configuration of the scope",
                        active.as_str()
                    ),
                );
            }
        }
        (None, false) => {}
    }
    check_active_arrangements(index, id, id, &scope.active_arrangements, findings);
    for (occurrence, design) in &scope.occurrences {
        let key = occurrence.as_str();
        if !index.occurrences.contains_key(key) {
            super::error(
                findings,
                id,
                format_args!(
                    "scope states the design of {key}, which is no occurrence of the model"
                ),
            );
            continue;
        }
        let prototype = index.prototype(key);
        if let Some(configuration) = &design.configuration {
            check_configuration_ref(index, configuration, prototype, id, findings);
        }
        check_arrangement_refs(index, &design.arrangements, prototype, id, findings);
        if !design.parameters.is_empty() && !prototype.is_some_and(|p| index.scopes.contains_key(p))
        {
            super::error(
                findings,
                id,
                format_args!("occurrence {key} overrides parameters, but its prototype has no scope in this document"),
            );
        }
        for (parameter, value) in &design.parameters {
            let of_prototype = index
                .parameters
                .get(parameter.as_str())
                .is_some_and(|p| Some(p.scope.as_str()) == prototype);
            if !of_prototype {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "occurrence {key} overrides {}, which is no parameter of its prototype's scope",
                        parameter.as_str()
                    ),
                );
            }
            check_parameter_override_fields(value, parameter.as_str(), id, findings);
        }
    }
}

/// A `ConfigurationRef::Local` names a configuration of the prototype's
/// scope; `Named` occurs only where that scope is absent or lacks the name.
pub(super) fn check_configuration_ref(
    index: &DesignRecords<'_>,
    configuration: &ConfigurationRef,
    prototype: Option<&str>,
    entity: &str,
    findings: &mut Vec<Finding>,
) {
    match configuration {
        ConfigurationRef::Local(local) => {
            let of_prototype = index
                .configurations
                .get(local.as_str())
                .is_some_and(|c| Some(c.scope.as_str()) == prototype);
            if !of_prototype {
                super::error(
                    findings,
                    entity,
                    format_args!(
                        "configuration reference {} names no configuration of the prototype's scope",
                        local.as_str()
                    ),
                );
            }
        }
        ConfigurationRef::Named(name) => {
            let local = prototype
                .and_then(|prototype| index.scope_configurations.get(prototype))
                .is_some_and(|configurations| {
                    configurations.iter().any(|c| c.name == name.as_str())
                });
            if local {
                super::error(
                    findings,
                    entity,
                    format_args!(
                        "configuration reference names `{}`, a configuration of the prototype's scope; it is `local`",
                        name.as_str()
                    ),
                );
            }
        }
    }
}

/// An occurrence's arrangements name at most one of each kind, and a `Local`
/// one names an arrangement of the prototype's scope.
fn check_arrangement_refs(
    index: &DesignRecords<'_>,
    arrangements: &[ArrangementRef],
    prototype: Option<&str>,
    entity: &str,
    findings: &mut Vec<Finding>,
) {
    let mut kinds: Vec<&ArrangementKind> = Vec::new();
    for arrangement in arrangements {
        let ArrangementRef::Local(local) = arrangement else {
            continue;
        };
        match index.arrangements.get(local.as_str()) {
            Some(record) if Some(record.scope.as_str()) == prototype => {
                if kinds.contains(&&record.kind) {
                    super::error(
                        findings,
                        entity,
                        format_args!("occurrence names two arrangements of one kind"),
                    );
                }
                kinds.push(&record.kind);
            }
            _ => super::error(
                findings,
                entity,
                format_args!(
                    "arrangement reference {} names no arrangement of the prototype's scope",
                    local.as_str()
                ),
            ),
        }
    }
}

/// `active_arrangements` names arrangements of the scope, at most one of each
/// kind.
fn check_active_arrangements(
    index: &DesignRecords<'_>,
    entity: &str,
    scope: &str,
    active: &BTreeSet<crate::design::ids::ArrangementId>,
    findings: &mut Vec<Finding>,
) {
    let mut kinds: Vec<&ArrangementKind> = Vec::new();
    for arrangement in active {
        match index.arrangements.get(arrangement.as_str()) {
            Some(record) if record.scope.as_str() == scope => {
                if kinds.contains(&&record.kind) {
                    super::error(
                        findings,
                        entity,
                        format_args!("active arrangements name two arrangements of one kind"),
                    );
                }
                kinds.push(&record.kind);
            }
            _ => super::error(
                findings,
                entity,
                format_args!(
                    "active arrangement {} is no arrangement of the scope",
                    arrangement.as_str()
                ),
            ),
        }
    }
}

/// An arrangement that names a configuration is in force in no other
/// configuration's resolved set.
fn check_arrangements_in_force(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    for configuration in &index.ir.design.configurations {
        let mut in_force: Vec<&Arrangement> = Vec::new();
        let base = index
            .scopes
            .get(configuration.scope.as_str())
            .map(|scope| &scope.active_arrangements);
        let sets = index
            .configuration_chain(Some(configuration.id.as_str()))
            .into_iter()
            .map(|configuration| &configuration.active_arrangements)
            .chain(base);
        for set in sets {
            for arrangement in set
                .iter()
                .filter_map(|id| index.arrangements.get(id.as_str()))
            {
                if !in_force.iter().any(|held| held.kind == arrangement.kind) {
                    in_force.push(arrangement);
                }
            }
        }
        for arrangement in in_force {
            if let Some(owner) = &arrangement.configuration {
                if *owner != configuration.id {
                    super::error(
                        findings,
                        arrangement.id.as_str(),
                        format_args!(
                            "arrangement of configuration {} is in force in configuration {}",
                            owner.as_str(),
                            configuration.id.as_str()
                        ),
                    );
                }
            }
        }
    }
}

fn check_arrangement(
    index: &DesignRecords<'_>,
    arrangement: &Arrangement,
    findings: &mut Vec<Finding>,
) {
    let id = arrangement.id.as_str();
    let scope = arrangement.scope.as_str();
    if !index.scopes.contains_key(scope) {
        super::error(
            findings,
            id,
            format_args!("arrangement's scope {scope} has no design scope"),
        );
    }
    if let Some(configuration) = &arrangement.configuration {
        let of_scope = index
            .configurations
            .get(configuration.as_str())
            .is_some_and(|c| c.scope == arrangement.scope);
        if !of_scope {
            super::error(
                findings,
                id,
                format_args!(
                    "arrangement names {}, which is no configuration of its scope",
                    configuration.as_str()
                ),
            );
        }
    }
    check_overrides(
        index,
        id,
        scope,
        &arrangement.overrides,
        Holder::Arrangement,
        findings,
    );
    if arrangement.kind == ArrangementKind::Explode
        && !(arrangement.overrides.parameters.is_empty()
            && arrangement.overrides.features.is_empty())
    {
        super::error(
            findings,
            id,
            format_args!("an explode arrangement states a parameter or feature override"),
        );
    }
    if matches!(
        arrangement.kind,
        ArrangementKind::Positional | ArrangementKind::Display
    ) && !arrangement.moves.is_empty()
    {
        super::error(
            findings,
            id,
            format_args!("arrangement states moves; only an explode arrangement moves occurrences"),
        );
    }
    let moved = moved_occurrences(index.ir);
    for appearance in &arrangement.appearances {
        if appearance.appearance.is_none() && appearance.transparency.is_none() {
            super::error(
                findings,
                id,
                format_args!("an appearance entry states neither an appearance nor a transparency"),
            );
        }
        check_path(index, Some(scope), &appearance.path, &moved, id, findings);
    }
    for movement in &arrangement.moves {
        if !matches!(
            movement.step,
            TransformStep::Translate { .. } | TransformStep::Rotate { .. }
        ) {
            super::error(
                findings,
                id,
                format_args!("an arrangement move is neither a `translate` nor a `rotate` step"),
            );
        }
        for path in &movement.occurrences {
            check_path(index, Some(scope), path, &moved, id, findings);
        }
    }
    for nested in &arrangement.nested {
        check_path(index, Some(scope), &nested.path, &moved, id, findings);
        if let ArrangementRef::Local(local) = &nested.arrangement {
            let prototype = last_prototype(index, &nested.path);
            let of_prototype = index
                .arrangements
                .get(local.as_str())
                .is_some_and(|record| Some(record.scope.as_str()) == prototype);
            if !of_prototype {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "nested arrangement {} is no arrangement of the path's prototype's scope",
                        local.as_str()
                    ),
                );
            }
        }
    }
}

fn check_overrides(
    index: &DesignRecords<'_>,
    holder: &str,
    scope: &str,
    overrides: &Overrides,
    kind: Holder,
    findings: &mut Vec<Finding>,
) {
    for (parameter, value) in &overrides.parameters {
        let key = parameter.as_str();
        match index.parameters.get(key) {
            None => super::error(
                findings,
                holder,
                format_args!("override names {key}, which is no parameter of the design"),
            ),
            Some(record) if !matches!(record.definition, ParameterDefinition::Expression(_)) => {
                super::error(
                    findings,
                    holder,
                    format_args!("override names {key}, which is not defined by an expression"),
                );
            }
            Some(_) => {}
        }
        check_parameter_override_fields(value, key, holder, findings);
    }
    for (feature, state) in &overrides.features {
        check_feature_override(index, holder, feature.as_str(), state, findings);
    }
    for (occurrence, value) in &overrides.occurrences {
        check_occurrence_override(index, holder, occurrence.as_str(), value, kind, findings);
    }
    if matches!(kind, Holder::Arrangement) && !overrides.definitions.is_empty() {
        super::error(
            findings,
            holder,
            format_args!("an arrangement's overrides hold a definition entry"),
        );
    }
    if overrides.definitions.is_empty() {
        return;
    }
    let spanned = index.spanned_definitions(scope);
    for (definition, value) in &overrides.definitions {
        check_definition_override(
            index,
            holder,
            &spanned,
            definition.as_str(),
            value,
            kind,
            findings,
        );
    }
}

fn check_parameter_override_fields(
    value: &ParameterOverride,
    parameter: &str,
    holder: &str,
    findings: &mut Vec<Finding>,
) {
    if value.expression.is_none() && value.value.is_none() {
        super::error(
            findings,
            holder,
            format_args!("override of {parameter} states neither an expression nor a value"),
        );
    }
}

fn check_feature_override(
    index: &DesignRecords<'_>,
    holder: &str,
    feature: &str,
    state: &FeatureStateOverride,
    findings: &mut Vec<Finding>,
) {
    if state.suppressed.is_none() && state.definition.is_none() {
        super::error(
            findings,
            holder,
            format_args!("feature-state override of {feature} states no field"),
        );
    }
    let Some(record) = index.features.get(feature) else {
        super::error(
            findings,
            holder,
            format_args!("override names {feature}, which is no feature of the design"),
        );
        return;
    };
    let bound = matches!(
        record.suppressed,
        Some(Dimension::Parameter(_) | Dimension::Negated(_))
    );
    if bound && state.suppressed.is_some() {
        super::error(
            findings,
            holder,
            format_args!(
                "override suppresses {feature}, whose suppression is bound to a parameter"
            ),
        );
    }
    let design = &index.ir.design;
    let owner = match state
        .definition
        .as_ref()
        .map(|definition| &definition.operation)
    {
        Some(FeatureOperation::Sketch(sketch)) => Some((
            sketch.as_str(),
            design
                .sketches
                .iter()
                .find(|record| record.id == *sketch)
                .map(|record| &record.owner),
        )),
        Some(FeatureOperation::SpatialSketch(sketch)) => Some((
            sketch.as_str(),
            design
                .spatial_sketches
                .iter()
                .find(|record| record.id == *sketch)
                .map(|record| &record.owner),
        )),
        _ => None,
    };
    if let Some((sketch, owner)) = owner {
        if owner != Some(&record.id) {
            super::error(
                findings,
                holder,
                format_args!("definition override of {feature} names sketch {sketch}, which the feature does not own"),
            );
        }
    }
}

fn check_occurrence_override(
    index: &DesignRecords<'_>,
    holder: &str,
    occurrence: &str,
    value: &OccurrenceOverride,
    kind: Holder,
    findings: &mut Vec<Finding>,
) {
    if !index.occurrences.contains_key(occurrence) {
        super::error(
            findings,
            holder,
            format_args!("override names {occurrence}, which is no occurrence of the model"),
        );
    }
    if matches!(kind, Holder::Arrangement)
        && (value.configuration.is_some() || value.prototype.is_some())
    {
        super::error(
            findings,
            holder,
            format_args!(
                "an arrangement's override of {occurrence} states a configuration or a prototype"
            ),
        );
    }
    if let Some(prototype) = &value.prototype {
        if !index.definitions.contains_key(prototype.as_str()) {
            super::error(
                findings,
                holder,
                format_args!(
                    "override of {occurrence} names prototype {}, which is no product definition",
                    prototype.as_str()
                ),
            );
        }
    }
    if let Some(configuration) = &value.configuration {
        let prototype = value
            .prototype
            .as_ref()
            .map(ProductDefinitionId::as_str)
            .or_else(|| index.prototype(occurrence));
        check_configuration_ref(index, configuration, prototype, holder, findings);
    }
    if let Some(name) = &value.representation {
        let prototype = value
            .prototype
            .as_ref()
            .map(ProductDefinitionId::as_str)
            .or_else(|| index.prototype(occurrence));
        check_representation_name(index, prototype, name.as_str(), holder, findings);
    }
    let mut paths = BTreeSet::new();
    let moved = moved_occurrences(index.ir);
    let prototype = value
        .prototype
        .as_ref()
        .map(ProductDefinitionId::as_str)
        .or_else(|| index.prototype(occurrence));
    for nested in &value.nested {
        check_path(index, prototype, &nested.path, &moved, holder, findings);
        check_unconfigured(&nested.path, holder, "a nested override's path", findings);
        if let Some(name) = &nested.representation {
            let prototype = last_prototype(index, &nested.path);
            check_representation_name(index, prototype, name.as_str(), holder, findings);
        }
        if !paths.insert(&nested.path) {
            super::error(
                findings,
                holder,
                format_args!("override of {occurrence} names one nested path twice"),
            );
        }
    }
}

fn check_definition_override(
    index: &DesignRecords<'_>,
    holder: &str,
    spanned: &BTreeSet<&str>,
    definition: &str,
    value: &DefinitionOverride,
    kind: Holder,
    findings: &mut Vec<Finding>,
) {
    let record = index.definitions.get(definition).copied();
    if record.is_none() || !spanned.contains(definition) {
        super::error(
            findings,
            holder,
            format_args!("override names {definition}, which is no definition the scope spans"),
        );
    }
    let states_field = value.source_name.is_some()
        || value.label.is_some()
        || value.description.is_some()
        || value.part_number.is_some()
        || !value.bom_properties.is_empty();
    if !states_field {
        super::error(
            findings,
            holder,
            format_args!("definition override of {definition} states no field"),
        );
    }
    match (&value.part_number, kind) {
        (Some(PartNumber::DocumentName), _) => {
            let title = NonBlankString::new("Title");
            let has_title = |properties: &BTreeMap<NonBlankString, String>| {
                title
                    .as_ref()
                    .is_some_and(|title| properties.contains_key(title))
            };
            let named = value.source_name.is_some()
                || has_title(&value.bom_properties)
                || record.is_some_and(|record| {
                    record.source_name.is_some() || has_title(&record.bom_properties)
                });
            if !named {
                super::error(
                    findings,
                    holder,
                    format_args!("part number of {definition} is the document name, but neither the definition nor the override states a `Title` or a source name"),
                );
            }
            let stored = record.is_some_and(|record| {
                matches!(record.storage, Storage::File { .. } | Storage::External(_))
            });
            if record.is_some() && !stored {
                super::error(
                    findings,
                    holder,
                    format_args!("part number of {definition} is the document name, but no document of its own stores it"),
                );
            }
        }
        (
            Some(PartNumber::ParentConfigurationName),
            Holder::Configuration { has_parent: false } | Holder::Arrangement,
        ) => {
            super::error(
                findings,
                holder,
                format_args!("part number of {definition} is the parent configuration's name, but no parent is stated"),
            );
        }
        (Some(PartNumber::ParentConfigurationName), Holder::Configuration { has_parent: true })
        | (Some(PartNumber::Text(_) | PartNumber::ConfigurationName) | None, _) => {}
    }
}

/// A parameter's `tolerance` and `decimals` occur only with a `Quantity`
/// kind; a `Fit` only with a length; every `Deviations` has its lower below
/// its upper.
fn check_parameter_tolerance(parameter: &DesignParameter, findings: &mut Vec<Finding>) {
    let id = parameter.id.as_str();
    let quantity = match parameter.kind {
        ValueKind::Quantity(quantity) => Some(quantity),
        ValueKind::Integer | ValueKind::Boolean | ValueKind::Text | ValueKind::Native(_) => None,
    };
    if parameter.decimals.is_some() && quantity.is_none() {
        super::error(
            findings,
            id,
            format_args!("parameter states `decimals` but its kind is no quantity"),
        );
    }
    let Some(tolerance) = &parameter.tolerance else {
        return;
    };
    if quantity.is_none() {
        super::error(
            findings,
            id,
            format_args!("parameter states a tolerance but its kind is no quantity"),
        );
    }
    let deviations = match tolerance {
        Tolerance::Deviations(deviations) => Some(deviations),
        Tolerance::Fit {
            fit,
            mate,
            deviations,
        } => {
            fits::check_mate_zone(id, fit, mate.as_ref(), findings);
            if quantity != Some(Quantity::LENGTH) {
                super::error(
                    findings,
                    id,
                    format_args!("parameter states a fit but its kind is no length"),
                );
            }
            deviations.as_ref().map(|deviations| match deviations {
                FitDeviations::Cell(deviations)
                | FitDeviations::Own(deviations)
                | FitDeviations::Unchecked(deviations) => deviations,
            })
        }
        Tolerance::General | Tolerance::Basic | Tolerance::SingleLimit(_) => None,
    };
    if let Some(Deviations { lower, upper }) = deviations {
        if lower.get() >= upper.get() {
            super::error(
                findings,
                id,
                format_args!(
                    "parameter's lower deviation {} is not below its upper deviation {}",
                    lower.get(),
                    upper.get()
                ),
            );
        }
    }
}

mod fits;

#[cfg(test)]
mod tests;
