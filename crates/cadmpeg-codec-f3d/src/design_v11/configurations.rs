// SPDX-License-Identifier: Apache-2.0
//! The configurations of a Fusion design (§5.11): one per row of its
//! configuration table.
//!
//! The model state the document stores is the state of the active row, so
//! the active row is the scope's active configuration and the base, and
//! states no overrides. Every other row states the expression of each
//! parameter its cells set, with the value the cell caches, and the
//! suppression of each feature its cells toggle. A scope whose active row
//! does not resolve states no configurations, since a scope with
//! configurations names its active one (part 10).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::design::configuration::{
    DesignConfiguration, FeatureStateOverride, Overrides, ParameterOverride,
};
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::features::{
    ConfigurationId, DesignConfiguration as LegacyConfiguration, ParameterId,
};
use cadmpeg_ir::ids::ProductDefinitionId;

use super::parameters::{cache, expression};
use super::timeline::document_prefix;

/// The configurations of the scopes keyed by document prefix in `scopes`,
/// and each scope's active configuration.
pub(super) struct Configurations {
    pub(super) records: Vec<DesignConfiguration>,
    pub(super) active: BTreeMap<ProductDefinitionId, ConfigurationId>,
}

/// `features` holds the ids of the scopes' features and `parameters` their
/// parameters.
pub(super) fn project(
    legacy: &[LegacyConfiguration],
    scopes: &BTreeMap<String, ProductDefinitionId>,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
) -> Configurations {
    let mut out = Configurations {
        records: Vec::new(),
        active: BTreeMap::new(),
    };
    for row in legacy {
        let home = row.native_ref.as_deref().unwrap_or(row.id.as_str());
        let Some(scope) = scopes.get(document_prefix(home)) else {
            continue;
        };
        let overrides = if row.active {
            out.active.insert(scope.clone(), row.id.clone());
            Overrides::default()
        } else {
            overrides(row, scope, features, parameters)
        };
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
    out.records
        .retain(|configuration| out.active.contains_key(&configuration.scope));
    out
}

fn overrides(
    row: &LegacyConfiguration,
    scope: &ProductDefinitionId,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
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
            continue;
        };
        overrides.parameters.insert(
            id.clone(),
            ParameterOverride {
                expression: Some(expression(text, &parameter.kind, &vocabulary)),
                value: row
                    .parameter_values
                    .get(id)
                    .and_then(|value| cache(value).1),
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
        }
    }
    overrides
}
