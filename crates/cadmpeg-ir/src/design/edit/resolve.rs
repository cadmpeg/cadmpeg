// SPDX-License-Identifier: Apache-2.0
//! Resolution through configurations (§5.11): a configuration's value is its
//! own where it states one, else its parent's resolved value, else the
//! base's.

use serde::de::DeserializeOwned;
use serde_json::Value as Json;

use super::EditError;
use crate::design::configuration::{
    Arrangement, DesignConfiguration, Overrides, ParameterOverride,
};
use crate::design::dimension::Dimension;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ops::body::PatternInstances;
use crate::design::ops::surface::MeshKind;
use crate::design::parameter::{DesignParameter, ParameterValue};
use crate::design::refs::ConfigurationRef;
use crate::design::scope::DesignScope;
use crate::design::selection::{OccurrencePath, StepRef};
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::CadIr;

impl CadIr {
    pub(super) fn design_feature(&self, id: &FeatureId) -> Result<&Feature, EditError> {
        self.design
            .features
            .iter()
            .find(|feature| &feature.id == id)
            .ok_or_else(|| EditError::UnknownFeature(id.clone()))
    }

    pub(super) fn design_parameter(&self, id: &ParameterId) -> Result<&DesignParameter, EditError> {
        self.design
            .parameters
            .iter()
            .find(|parameter| &parameter.id == id)
            .ok_or_else(|| EditError::UnknownParameter(id.clone()))
    }

    pub(super) fn design_scope(&self, definition: &ProductDefinitionId) -> Option<&DesignScope> {
        self.design
            .scopes
            .iter()
            .find(|scope| &scope.definition == definition)
    }

    /// The configuration and its ancestors, nearest first.
    pub(super) fn configuration_chain(
        &self,
        configuration: &ConfigurationId,
    ) -> Result<Vec<&DesignConfiguration>, EditError> {
        let mut chain: Vec<&DesignConfiguration> = Vec::new();
        let mut next = Some(configuration);
        while let Some(id) = next {
            if chain.iter().any(|seen| &seen.id == id) {
                return Err(EditError::ConfigurationCycle(id.clone()));
            }
            let found = self
                .design
                .configurations
                .iter()
                .find(|candidate| &candidate.id == id)
                .ok_or_else(|| EditError::UnknownConfiguration(id.clone()))?;
            chain.push(found);
            next = found.parent.as_ref();
        }
        Ok(chain)
    }

    /// The override of a parameter that applies in a configuration: the
    /// nearest of the configuration and its ancestors that states one.
    pub(super) fn parameter_override_in(
        &self,
        parameter: &ParameterId,
        configuration: Option<&ConfigurationId>,
    ) -> Result<Option<&ParameterOverride>, EditError> {
        let Some(configuration) = configuration else {
            return Ok(None);
        };
        Ok(self
            .configuration_chain(configuration)?
            .into_iter()
            .find_map(|level| level.overrides.parameters.get(parameter)))
    }

    /// A parameter's cache in a configuration: the value of the override that
    /// applies there, else the parameter's own value. `None` in `configuration`
    /// reads the base.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownParameter`] or
    /// [`EditError::UnknownConfiguration`] for an id the design does not hold,
    /// and [`EditError::ConfigurationCycle`] where the configuration's
    /// parents form a cycle.
    pub fn parameter_cache(
        &self,
        parameter: &ParameterId,
        configuration: Option<&ConfigurationId>,
    ) -> Result<Option<&ParameterValue>, EditError> {
        let record = self.design_parameter(parameter)?;
        Ok(
            match self.parameter_override_in(parameter, configuration)? {
                Some(applied) => applied.value.as_ref(),
                None => record.value.as_ref(),
            },
        )
    }

    /// The configuration that the current geometry holds for a definition's
    /// scope; `None` where the definition has no scope or the scope has no
    /// configurations.
    pub(super) fn active_configuration_of(
        &self,
        definition: &ProductDefinitionId,
    ) -> Option<&ConfigurationId> {
        self.design_scope(definition)
            .and_then(|scope| scope.active_configuration.as_ref())
    }

    /// The arrangements in force in a configuration of a scope, at most one
    /// of each kind: per kind, the one the configuration names, else its
    /// parent's resolved one, else the one the scope's base names. `None` in
    /// `configuration` reads the base.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownConfiguration`] for a configuration the
    /// design does not hold, and [`EditError::ConfigurationCycle`] where the
    /// configuration's parents form a cycle.
    pub(super) fn arrangements_in_force(
        &self,
        scope: &ProductDefinitionId,
        configuration: Option<&ConfigurationId>,
    ) -> Result<Vec<&Arrangement>, EditError> {
        let chain = match configuration {
            Some(configuration) => self.configuration_chain(configuration)?,
            None => Vec::new(),
        };
        let base = self
            .design_scope(scope)
            .map(|record| &record.active_arrangements);
        let mut in_force: Vec<&Arrangement> = Vec::new();
        for named in chain
            .iter()
            .map(|level| &level.active_arrangements)
            .chain(base)
        {
            for id in named {
                let Some(arrangement) = self
                    .design
                    .arrangements
                    .iter()
                    .find(|candidate| &candidate.id == id)
                else {
                    continue;
                };
                if !in_force.iter().any(|held| held.kind == arrangement.kind) {
                    in_force.push(arrangement);
                }
            }
        }
        Ok(in_force)
    }

    /// The configuration of its prototype that an occurrence of `scope` uses:
    /// its `OccurrenceDesign.configuration` resolved through the scope's
    /// active configuration and then through the arrangements in force in
    /// it (§5.11).
    ///
    /// # Errors
    ///
    /// As [`CadIr::arrangements_in_force`].
    pub(super) fn occurrence_configuration(
        &self,
        scope: &DesignScope,
        occurrence: &OccurrenceId,
    ) -> Result<Option<ConfigurationRef>, EditError> {
        self.occurrence_configuration_in(scope, occurrence, scope.active_configuration.as_ref())
    }

    /// As [`CadIr::occurrence_configuration`], with `active` the
    /// configuration of the scope that applies in place of the scope's
    /// active one: the configuration an ancestor occurrence selects.
    fn occurrence_configuration_in(
        &self,
        scope: &DesignScope,
        occurrence: &OccurrenceId,
        active: Option<&ConfigurationId>,
    ) -> Result<Option<ConfigurationRef>, EditError> {
        let chain = match active {
            Some(configuration) => self.configuration_chain(configuration)?,
            None => Vec::new(),
        };
        let stated = |overrides: &Overrides| {
            overrides
                .occurrences
                .get(occurrence)
                .and_then(|state| state.configuration.clone())
        };
        let arranged = self
            .arrangements_in_force(&scope.definition, active)?
            .into_iter()
            .find_map(|arrangement| stated(&arrangement.overrides));
        let configured = chain.iter().find_map(|level| stated(&level.overrides));
        Ok(arranged.or(configured).or_else(|| {
            scope
                .occurrences
                .get(occurrence)
                .and_then(|design| design.configuration.clone())
        }))
    }

    /// The cache a reference `through` an occurrence path reads (§5.13): the
    /// occurrence the path reaches states its own override of the parameter
    /// (`OccurrenceDesign.parameters`, §5.1), else the parameter's cache in
    /// the configuration that occurrence uses. Each step uses its own
    /// `configuration`, else the one its scope resolves for it (§5.1) in the
    /// configuration the step above uses, the first step in its scope's
    /// active one, else its prototype's active configuration. `None` where
    /// no cache is stated: an empty path, a `Native` step, and an occurrence
    /// whose configuration is `Named` or whose override states no value.
    ///
    /// # Errors
    ///
    /// As [`CadIr::parameter_cache`].
    pub fn through_cache(
        &self,
        parameter: &ParameterId,
        path: &OccurrencePath,
    ) -> Result<Option<&ParameterValue>, EditError> {
        let mut level: Option<Option<ConfigurationId>> = None;
        let mut reached = None;
        for step in &path.0 {
            let StepRef::Occurrence(occurrence) = &step.occurrence else {
                return Ok(None);
            };
            let scope = self
                .design
                .scopes
                .iter()
                .find(|scope| scope.occurrences.contains_key(occurrence));
            let resolved = match (&step.configuration, scope) {
                (Some(stated), _) => Some(stated.clone()),
                (None, Some(scope)) => {
                    let active = match &level {
                        Some(selected) => selected.as_ref(),
                        None => scope.active_configuration.as_ref(),
                    };
                    self.occurrence_configuration_in(scope, occurrence, active)?
                }
                (None, None) => None,
            };
            let uses = resolved.or_else(|| {
                self.model
                    .occurrences
                    .iter()
                    .find(|candidate| &candidate.id == occurrence)
                    .and_then(|record| self.active_configuration_of(&record.prototype))
                    .cloned()
                    .map(ConfigurationRef::Local)
            });
            level = Some(match &uses {
                Some(ConfigurationRef::Local(id)) => Some(id.clone()),
                Some(ConfigurationRef::Named(_)) | None => None,
            });
            reached = Some((occurrence, scope, uses));
        }
        let Some((occurrence, scope, uses)) = reached else {
            return Ok(None);
        };
        if let Some(own) = scope
            .and_then(|scope| scope.occurrences.get(occurrence))
            .and_then(|design| design.parameters.get(parameter))
        {
            return Ok(own.value.as_ref());
        }
        match uses {
            Some(ConfigurationRef::Named(_)) => Ok(None),
            Some(ConfigurationRef::Local(configuration)) => {
                self.parameter_cache(parameter, Some(&configuration))
            }
            None => self.parameter_cache(parameter, None),
        }
    }

    /// Resolves a dimension: a literal is its own value, and a bound
    /// dimension takes its parameter's cache in the active configuration of
    /// the parameter's scope (§5.14), negated for `Negated`. `None` is a
    /// bound dimension whose parameter holds no cache there.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownParameter`] for a parameter the design
    /// does not hold, and [`EditError::DimensionValue`] where the cache does
    /// not read as a value of `T`, or `Negated` binds a value that has no
    /// negation.
    pub fn dimension_value<T: Clone + DeserializeOwned>(
        &self,
        dimension: &Dimension<T>,
    ) -> Result<Option<T>, EditError> {
        let (parameter, negated) = match dimension {
            Dimension::Literal(value) => return Ok(Some(value.clone())),
            Dimension::Parameter(parameter) => (parameter, false),
            Dimension::Negated(parameter) => (parameter, true),
        };
        let scope = &self.design_parameter(parameter)?.scope;
        let configuration = self.active_configuration_of(scope);
        let Some(cache) = self.parameter_cache(parameter, configuration)? else {
            return Ok(None);
        };
        cache_as(parameter, cache, negated).map(Some)
    }

    fn feature_suppressed_in(
        &self,
        feature: &Feature,
        chain: &[&DesignConfiguration],
        configuration: Option<&ConfigurationId>,
    ) -> Result<bool, EditError> {
        if let Some(own) = chain.iter().find_map(|level| {
            level
                .overrides
                .features
                .get(&feature.id)
                .and_then(|state| state.suppressed)
        }) {
            return Ok(own);
        }
        let (parameter, negated) = match &feature.suppressed {
            None => return Ok(false),
            Some(Dimension::Literal(value)) => return Ok(*value),
            Some(Dimension::Parameter(parameter)) => (parameter, false),
            Some(Dimension::Negated(parameter)) => (parameter, true),
        };
        Ok(match self.parameter_cache(parameter, configuration)? {
            Some(ParameterValue::Boolean(value)) => *value != negated,
            _ => false,
        })
    }

    /// The definition of a feature that applies in a configuration: the
    /// nearest override that states one, else the feature's own.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownFeature`] or
    /// [`EditError::UnknownConfiguration`] for an id the design does not
    /// hold, and [`EditError::ConfigurationCycle`] where the configuration's
    /// parents form a cycle.
    pub fn feature_definition_in(
        &self,
        feature: &FeatureId,
        configuration: Option<&ConfigurationId>,
    ) -> Result<&FeatureDefinition, EditError> {
        let record = self.design_feature(feature)?;
        let chain = match configuration {
            Some(configuration) => self.configuration_chain(configuration)?,
            None => Vec::new(),
        };
        Ok(chain
            .iter()
            .find_map(|level| {
                level
                    .overrides
                    .features
                    .get(feature)
                    .and_then(|state| state.definition.as_ref())
            })
            .unwrap_or(&record.definition))
    }

    /// Whether feature `feature` is stateless in configuration
    /// `configuration` (`None` in a scope with no configurations, §5.4): its
    /// `suppressed` resolves to true there, or the operation of the
    /// definition that applies there cannot change a state. A suppression
    /// bound to a parameter that holds no Boolean cache there does not
    /// resolve to true.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownFeature`] or
    /// [`EditError::UnknownConfiguration`] for an id the design does not
    /// hold, and [`EditError::ConfigurationCycle`] where the configuration's
    /// parents form a cycle.
    pub fn is_stateless(
        &self,
        feature: &FeatureId,
        configuration: Option<&ConfigurationId>,
    ) -> Result<bool, EditError> {
        let record = self.design_feature(feature)?;
        let chain = match configuration {
            Some(configuration) => self.configuration_chain(configuration)?,
            None => Vec::new(),
        };
        if self.feature_suppressed_in(record, &chain, configuration)? {
            return Ok(true);
        }
        let definition = self.feature_definition_in(feature, configuration)?;
        Ok(stateless_operation(&definition.operation))
    }
}

/// Whether an operation's kind cannot change a state: a sketch, a sheet, a
/// datum, a feature that only generates other features, an appearance, and a
/// section that only writes curves into a sketch.
pub(super) fn stateless_operation(operation: &FeatureOperation) -> bool {
    match operation {
        FeatureOperation::Sketch(_)
        | FeatureOperation::SpatialSketch(_)
        | FeatureOperation::Spreadsheet(_)
        | FeatureOperation::Datum(_)
        | FeatureOperation::TemplateInstance(_)
        | FeatureOperation::Decal(_) => true,
        FeatureOperation::Pattern(pattern) => {
            matches!(pattern.instances, PatternInstances::Members(_))
        }
        FeatureOperation::Mesh(mesh) => matches!(mesh.kind, MeshKind::Section { .. }),
        _ => false,
    }
}

/// Reads a cache as a value of `T`, negated where `negated` holds: a number
/// changes sign and a Boolean is complemented. A text is read as a string
/// first, then as the unit arm it names, then as the `Native` arm holding it.
fn cache_as<T: DeserializeOwned>(
    parameter: &ParameterId,
    cache: &ParameterValue,
    negated: bool,
) -> Result<T, EditError> {
    let refusal = |reason: String| EditError::DimensionValue {
        parameter: parameter.clone(),
        reason,
    };
    let json = match (cache, negated) {
        (ParameterValue::Number(value), false) => Json::from(value.get()),
        (ParameterValue::Number(value), true) => Json::from(-value.get() + 0.0),
        (ParameterValue::Integer(value), false) => Json::from(*value),
        (ParameterValue::Boolean(value), negated) => Json::from(*value != negated),
        (ParameterValue::Text(text), false) => {
            let forms = [
                Json::from(text.as_str()),
                serde_json::json!({ "kind": text }),
                serde_json::json!({ "kind": "native", "value": text }),
            ];
            let mut last = None;
            for form in forms {
                match serde_json::from_value::<T>(form) {
                    Ok(value) => return Ok(value),
                    Err(error) => last = Some(error),
                }
            }
            return Err(refusal(
                last.map_or_else(String::new, |error| error.to_string()),
            ));
        }
        (ParameterValue::Integer(_) | ParameterValue::Text(_), true) => {
            return Err(refusal(
                "a negated binding takes a signed quantity or a Boolean".into(),
            ));
        }
    };
    serde_json::from_value(json).map_err(|error| refusal(error.to_string()))
}
