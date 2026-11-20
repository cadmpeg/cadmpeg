// SPDX-License-Identifier: Apache-2.0
//! The configurations of a Fusion design (§5.11): one per row of its
//! configuration table.
//!
//! The model state the document stores is the state of the active row, so
//! the active row is the scope's active configuration and the base, and
//! states no overrides. Every other row states the expression of each
//! parameter its cells set, with the value the cell caches typed as the
//! parameter's value, and the suppression of each feature its cells toggle.
//! Its `Part Number` and `Description` property cells are the part number and
//! the description of the scope's definition (§5.11, Identity carriers); an
//! empty part-number cell is the configuration's name. A scope whose active
//! row does not resolve states no configurations, since a scope with
//! configurations names its active one (part 10). The rows of a document
//! with no scope of its own state no configuration. A loss names each other
//! property of a configuration the design states, each parameter cell whose
//! cached value the design does not state, and each cell whose target the
//! design does not state, the unresolved targets a row keeps as properties
//! among them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::design::configuration::{
    DesignConfiguration, FeatureStateOverride, Overrides, ParameterOverride, PartNumber,
};
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::features::{
    ConfigurationId, DesignConfiguration as LegacyConfiguration, ParameterId,
};
use cadmpeg_ir::ids::ProductDefinitionId;

use cadmpeg_ir::report::loss::LossNote;

use super::parameters::{cache, expression};
use super::timeline::document_prefix;
use crate::loss::F3dLossCode;

/// The configurations of the scopes keyed by document prefix in `scopes`,
/// and each scope's active configuration.
pub(super) struct Configurations {
    pub(super) records: Vec<DesignConfiguration>,
    pub(super) active: BTreeMap<ProductDefinitionId, ConfigurationId>,
}

/// A document in `hosted`, or one `scopes` does not key, has no scope of its
/// own. `features` holds the ids of the scopes' features and `parameters`
/// their parameters. Each class of row or cell the design omits charges one
/// loss to `losses`.
pub(super) fn project(
    legacy: &[LegacyConfiguration],
    scopes: &BTreeMap<String, ProductDefinitionId>,
    hosted: &BTreeSet<String>,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
    losses: &mut Vec<LossNote>,
) -> Configurations {
    let mut out = Configurations {
        records: Vec::new(),
        active: BTreeMap::new(),
    };
    let mut unscoped = Vec::new();
    let mut placed = Vec::new();
    for row in legacy {
        let prefix = document_prefix(row.native_ref.as_deref().unwrap_or(row.id.as_str()));
        match scopes.get(prefix) {
            Some(scope) if !hosted.contains(prefix) => {
                if row.active {
                    out.active.insert(scope.clone(), row.id.clone());
                }
                placed.push((row, scope));
            }
            Some(_) | None => unscoped.push(row.id.as_str()),
        }
    }
    let mut inactive = Vec::new();
    let mut unstated = Vec::new();
    let mut uncached = Vec::new();
    let mut properties = Vec::new();
    for (row, scope) in placed {
        if !out.active.contains_key(scope) {
            inactive.push(row.id.as_str());
            continue;
        }
        let mut overrides = if row.active {
            Overrides::default()
        } else {
            overrides(
                row,
                scope,
                features,
                parameters,
                &mut unstated,
                &mut uncached,
            )
        };
        for (key, text) in &row.properties {
            let named = format!("{key} in {}", row.id);
            if ["suppressed:", "parameter:"]
                .iter()
                .any(|prefix| key.as_str().starts_with(prefix))
            {
                unstated.push(named);
            } else if row.active || !identity(&mut overrides, scope, key.as_str(), text) {
                properties.push(named);
            }
        }
        out.records.push(DesignConfiguration {
            id: row.id.clone(),
            scope: scope.clone(),
            name: row.name.clone().unwrap_or_default(),
            parent: None,
            comment: None,
            overrides,
            active_arrangements: BTreeSet::new(),
            materials: Vec::new(),
            native_ref: row.native_ref.clone(),
        });
    }
    for (code, count, what, named) in [
        (
            F3dLossCode::ConfigurationReferencedDesignUnscoped,
            unscoped.len(),
            "configuration row(s) of documents with no design scope of their own stay v10 configurations",
            unscoped.join(" "),
        ),
        (
            F3dLossCode::ConfigurationScopeWithoutActiveRow,
            inactive.len(),
            "configuration row(s) of scopes whose active row does not resolve stay v10 configurations",
            inactive.join(" "),
        ),
        (
            F3dLossCode::ConfigurationOverrideUnstated,
            unstated.len(),
            "configuration cell(s) set a parameter or feature the design does not state, so their rows omit them",
            unstated.join("; "),
        ),
        (
            F3dLossCode::ConfigurationOverrideValueUnprojected,
            uncached.len(),
            "configuration cell(s) cache a value the design does not state, so their overrides state only the expression",
            uncached.join("; "),
        ),
        (
            F3dLossCode::ConfigurationPropertyUnprojected,
            properties.len(),
            "configuration propert(ies) the design does not state, so their rows omit them",
            properties.join("; "),
        ),
    ] {
        if count != 0 {
            losses.push(code.note(format!("{count} {what}: {named}")));
        }
    }
    out
}

/// States in `overrides` the identity field of `scope`'s definition that the
/// configuration property `key` sets to `text`; false where `key` sets none.
fn identity(overrides: &mut Overrides, scope: &ProductDefinitionId, key: &str, text: &str) -> bool {
    let definition = overrides.definitions.entry(scope.clone());
    match key {
        "property:Part Number" => {
            definition.or_default().part_number = Some(if text.is_empty() {
                PartNumber::ConfigurationName
            } else {
                PartNumber::Text(text.to_owned())
            });
        }
        "property:Description" => definition.or_default().description = Some(text.to_owned()),
        _ => return false,
    }
    true
}

/// The overrides of `row`'s cells; each cell that sets a parameter or feature
/// the design does not state is named in `unstated`, and each parameter cell
/// whose cached value the design does not state in `uncached`.
fn overrides(
    row: &LegacyConfiguration,
    scope: &ProductDefinitionId,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
    unstated: &mut Vec<String>,
    uncached: &mut Vec<String>,
) -> Overrides {
    let scoped = parameters
        .iter()
        .filter(|parameter| &parameter.scope == scope)
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let vocabulary = scoped
        .values()
        .filter_map(|parameter| Some((parameter.name.as_ref()?.as_str(), &parameter.id)))
        .collect::<HashMap<&str, &ParameterId>>();
    let mut overrides = Overrides::default();
    for (id, text) in &row.parameter_overrides {
        let Some(parameter) = scoped.get(id) else {
            unstated.push(format!("{id} in {}", row.id));
            continue;
        };
        let value = row
            .parameter_values
            .get(id)
            .and_then(|value| cache(value).1);
        if value.is_none() {
            uncached.push(format!("{id} in {}", row.id));
        }
        overrides.parameters.insert(
            id.clone(),
            ParameterOverride {
                expression: Some(expression(text, &parameter.kind, &vocabulary)),
                value,
            },
        );
    }
    for (id, state) in &row.feature_states {
        if features.contains(id.as_str()) {
            overrides.features.insert(
                id.clone(),
                FeatureStateOverride {
                    suppressed: Some(state.evaluation.is_suppressed()),
                    definition: None,
                },
            );
        } else {
            unstated.push(format!("{id} in {}", row.id));
        }
    }
    overrides
}
