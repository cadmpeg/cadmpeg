// SPDX-License-Identifier: Apache-2.0
//! Canonical design edits and explicit evaluation invalidation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use super::CadIr;
use crate::features::{
    Dimension, DimensionResolutionError, DimensionValue, Expression, FeatureDefinition, FeatureId,
    ParameterId, ParameterValue,
};
use crate::schema::rewrite::typed::RewriteIdentities;

/// A canonical design edit that could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DimensionEditError {
    /// The requested feature is absent.
    UnknownFeature {
        /// Missing feature identity.
        feature: FeatureId,
    },
    /// A requested or referenced parameter is absent.
    UnknownParameter {
        /// Missing canonical parameter identity.
        parameter: ParameterId,
    },
    /// The JSON pointer does not select a dimensional field in the definition.
    InvalidPath {
        /// Unresolved JSON pointer.
        path: String,
    },
    /// The value cannot satisfy the selected quantity's admission rules.
    InvalidDimension {
        /// Quantity or relation admission detail.
        reason: String,
    },
    /// Changing a driven dimension requires editing its drivers or expression.
    DrivenParameter {
        /// Canonical parameter whose formula must be preserved.
        parameter: ParameterId,
        /// Parameters referenced by the current expression.
        drivers: Vec<ParameterId>,
    },
}
impl std::fmt::Display for DimensionEditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFeature { feature } => write!(formatter, "unknown feature {feature}"),
            Self::UnknownParameter { parameter } => {
                write!(formatter, "unknown parameter {parameter}")
            }
            Self::InvalidPath { path } => write!(formatter, "not a dimension path: {path}"),
            Self::InvalidDimension { reason } => formatter.write_str(reason),
            Self::DrivenParameter { parameter, drivers } => write!(
                formatter,
                "parameter {parameter} is driven by {} parameter(s)",
                drivers.len()
            ),
        }
    }
}
impl std::error::Error for DimensionEditError {}

impl CadIr {
    /// Resolve dimensional bindings into a temporary native-encoder projection.
    /// The returned JSON contains scalar literal payloads, not duplicated state.
    pub fn resolved_feature_definition(
        &self,
        definition: &FeatureDefinition,
    ) -> Result<Value, DimensionResolutionError> {
        resolve_encoder_value(self, definition)
    }

    /// Resolve a sketch constraint's dimensions for a native encoder.
    pub fn resolved_sketch_constraint_definition(
        &self,
        definition: &crate::sketches::SketchConstraintDefinitionInput,
    ) -> Result<Value, DimensionResolutionError> {
        let value = resolve_encoder_value(self, definition)?;
        Ok(value)
    }

    /// Resolve a dimension from its literal or its parameter's current cache.
    pub fn dimension_value<Q: DimensionValue>(&self, dimension: &Dimension<Q>) -> Option<Q> {
        dimension.value_from(&self.model.parameters)
    }

    /// Change a dimension selected by a JSON pointer relative to the feature definition.
    /// Bound dimensions edit their parameter; driven parameters are never overwritten.
    pub fn set_dimension<Q: DimensionValue + Serialize + DeserializeOwned>(
        &mut self,
        feature: &FeatureId,
        path: &str,
        value: Q,
    ) -> Result<(), DimensionEditError> {
        let ordinal = self
            .model
            .features
            .iter()
            .position(|row| row.id == *feature)
            .ok_or_else(|| DimensionEditError::UnknownFeature {
                feature: feature.clone(),
            })?;
        let mut definition =
            serde_json::to_value(self.model.features[ordinal].definition()).map_err(invalid)?;
        let selected =
            definition
                .pointer_mut(path)
                .ok_or_else(|| DimensionEditError::InvalidPath {
                    path: path.to_owned(),
                })?;
        let dimension: Dimension<Q> = serde_json::from_value(selected.clone()).map_err(|_| {
            DimensionEditError::InvalidPath {
                path: path.to_owned(),
            }
        })?;
        match dimension {
            Dimension::Parameter(parameter) => {
                let current = self
                    .model
                    .parameters
                    .iter()
                    .find(|row| row.id == parameter)
                    .ok_or_else(|| DimensionEditError::UnknownParameter {
                        parameter: parameter.clone(),
                    })?;
                let drivers = current
                    .expression
                    .dependencies()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                if !current.driving || !drivers.is_empty() {
                    return Err(DimensionEditError::DrivenParameter { parameter, drivers });
                }
                let value = value.to_parameter_value().ok_or_else(|| {
                    DimensionEditError::InvalidDimension {
                        reason: "dimension cannot be represented as a parameter value".into(),
                    }
                })?;
                // The bound wire stores no quantity tag. Validate the new cache
                // through the concrete Dimension<Q> slot before committing it.
                let parameter_index = self
                    .model
                    .parameters
                    .iter()
                    .position(|row| row.id == parameter)
                    .ok_or_else(|| DimensionEditError::UnknownParameter {
                        parameter: parameter.clone(),
                    })?;
                let previous = self.model.parameters[parameter_index]
                    .value
                    .replace(value.clone());
                let admission = (|| {
                    let arena = DecodeArena::new();
                    let policy = DecodePolicy::default();
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).map_err(invalid)?;
                    let mut errors = Vec::new();
                    self.model.features[ordinal]
                        .definition()
                        .validate_dimensions(self, &ctx, &mut errors)
                        .map_err(|limit| invalid(cadmpeg_core::CodecError::ResourceLimit(limit)))?;
                    if let Some(error) = errors.into_iter().find(|error| matches!(error,
                        DimensionResolutionError::WrongQuantity { parameter: id, .. } if *id == parameter)) {
                        return Err(invalid(error));
                    }
                    match self.model.features[ordinal]
                        .definition()
                        .clone()
                        .resolve_dimensions(self, &ctx)
                    {
                        Ok(resolved) => {
                            let value = serde_json::to_value(resolved).map_err(invalid)?;
                            serde_json::from_value::<FeatureDefinition>(value).map_err(invalid)?;
                        }
                        Err(DimensionResolutionError::UnevaluatedParameter { .. }) => {}
                        Err(error) => return Err(invalid(error)),
                    }
                    Ok(())
                })();
                self.model.parameters[parameter_index].value = previous;
                admission?;
                self.set_parameter_expression(
                    &parameter,
                    Expression::literal(&value),
                    Some(value),
                )?;
                self.mark_needs_recompute(feature)
            }
            Dimension::Literal(_) => {
                if let Some(cache) = value.to_parameter_value() {
                    // Literal scalar JSON carries no unit tag. A temporary
                    // binding lets the concrete typed slot admit the quantity
                    // without adding any authored document state.
                    let mut suffix = 0_u64;
                    let probe = loop {
                        let id =
                            ParameterId::mint(format!("cadmpeg:edit:parameter#quantity-{suffix}"))
                                .map_err(invalid)?;
                        if !self.model.parameters.iter().any(|row| row.id == id) {
                            break id;
                        }
                        suffix = suffix
                            .checked_add(1)
                            .ok_or_else(|| invalid("dimension probe identity capacity exceeded"))?;
                    };
                    *selected = serde_json::to_value(Dimension::<Q>::Parameter(probe.clone()))
                        .map_err(invalid)?;
                    let probe_definition: FeatureDefinition =
                        serde_json::from_value(definition.clone()).map_err(invalid)?;
                    self.model
                        .parameters
                        .push(crate::features::DesignParameter {
                            id: probe,
                            owner: None,
                            ordinal: 0,
                            name: String::new(),
                            expression: Expression::literal(&cache),
                            driving: true,
                            display: None,
                            value: Some(cache),
                            properties: BTreeMap::new(),
                            pmi: None,
                            native_ref: None,
                        });
                    let admission = (|| {
                        let arena = DecodeArena::new();
                        let policy = DecodePolicy::default();
                        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                            .map_err(invalid)?;
                        let mut errors = Vec::new();
                        probe_definition
                            .validate_dimensions(self, &ctx, &mut errors)
                            .map_err(|limit| {
                                invalid(cadmpeg_core::CodecError::ResourceLimit(limit))
                            })?;
                        if let Some(error) = errors.into_iter().next() {
                            return Err(invalid(error));
                        }
                        Ok(())
                    })();
                    self.model.parameters.pop();
                    admission?;
                }
                let selected = definition.pointer_mut(path).ok_or_else(|| {
                    DimensionEditError::InvalidPath {
                        path: path.to_owned(),
                    }
                })?;
                *selected = serde_json::to_value(Dimension::Literal(value)).map_err(invalid)?;
                let definition: FeatureDefinition =
                    serde_json::from_value(definition).map_err(invalid)?;
                self.set_feature_definition(feature, definition)
            }
        }
    }

    /// Replace a canonical parameter expression and its optional evaluated cache.
    pub fn set_parameter_expression(
        &mut self,
        parameter: &ParameterId,
        expression: Expression,
        value: Option<ParameterValue>,
    ) -> Result<(), DimensionEditError> {
        let ordinal = self
            .model
            .parameters
            .iter()
            .position(|row| row.id == *parameter)
            .ok_or_else(|| DimensionEditError::UnknownParameter {
                parameter: parameter.clone(),
            })?;
        for reference in expression.dependencies() {
            if !self.model.parameters.iter().any(|row| row.id == *reference) {
                return Err(DimensionEditError::UnknownParameter {
                    parameter: reference.clone(),
                });
            }
        }
        let affected = self.affected_evaluations([parameter.as_str()])?;
        self.model.parameters[ordinal].expression = expression;
        self.model.parameters[ordinal].value = value;
        self.invalidate(&affected);
        Ok(())
    }

    /// Replace a feature definition and mark its stored evaluation and dependents stale.
    pub fn set_feature_definition(
        &mut self,
        feature: &FeatureId,
        definition: FeatureDefinition,
    ) -> Result<(), DimensionEditError> {
        let ordinal = self
            .model
            .features
            .iter()
            .position(|row| row.id == *feature)
            .ok_or_else(|| DimensionEditError::UnknownFeature {
                feature: feature.clone(),
            })?;
        let affected = self.affected_evaluations([feature.as_str()])?;
        self.model.features[ordinal]
            .evaluation
            .set_definition(definition);
        self.invalidate(&affected);
        Ok(())
    }

    /// Mark a changed feature or referent's stored evaluation and all consumers stale.
    pub fn mark_needs_recompute(&mut self, feature: &FeatureId) -> Result<(), DimensionEditError> {
        if !self.model.features.iter().any(|row| row.id == *feature) {
            return Err(DimensionEditError::UnknownFeature {
                feature: feature.clone(),
            });
        }
        let affected = self.affected_evaluations([feature.as_str()])?;
        self.invalidate(&affected);
        Ok(())
    }

    fn invalidate(&mut self, affected: &BTreeSet<String>) {
        for feature in &mut self.model.features {
            if affected.contains(feature.id.as_str()) {
                feature.needs_recompute = true;
            }
        }
    }

    fn affected_evaluations<'a>(
        &self,
        seeds: impl IntoIterator<Item = &'a str>,
    ) -> Result<BTreeSet<String>, DimensionEditError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .map_err(invalid)?;
        let mut consumers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for parameter in &self.model.parameters {
            for driver in parameter.expression.dependencies() {
                consumers
                    .entry(driver.to_string())
                    .or_default()
                    .insert(parameter.id.to_string());
            }
            if let Some(owner) = &parameter.owner {
                consumers
                    .entry(parameter.id.to_string())
                    .or_default()
                    .insert(owner.to_string());
            }
        }
        for constraint in &self.model.sketch_constraints {
            for parameter in constraint
                .definition
                .kind()
                .dimension_parameters()
                .map_err(invalid)?
            {
                consumers
                    .entry(parameter.to_string())
                    .or_default()
                    .insert(constraint.sketch.to_string());
            }
        }
        for constraint in &self.model.spatial_sketch_constraints {
            for parameter in constraint
                .definition
                .kind()
                .dimension_parameters()
                .map_err(invalid)?
            {
                consumers
                    .entry(parameter.to_string())
                    .or_default()
                    .insert(constraint.sketch.to_string());
            }
        }
        for feature in &self.model.features {
            for dependency in &feature.dependencies {
                consumers
                    .entry(dependency.to_string())
                    .or_default()
                    .insert(feature.id.to_string());
            }
            if let Some(parent) = self.model.feature_regeneration_parent(&feature.id) {
                consumers
                    .entry(parent.to_string())
                    .or_default()
                    .insert(feature.id.to_string());
            }
            feature
                .definition()
                .visit_identity_references(&ctx, &mut |reference| {
                    consumers
                        .entry(reference.to_owned())
                        .or_default()
                        .insert(feature.id.to_string());
                    Ok(())
                })
                .map_err(invalid)?;
        }
        let mut visited = BTreeSet::new();
        let mut pending = seeds
            .into_iter()
            .map(str::to_owned)
            .collect::<VecDeque<_>>();
        while let Some(id) = pending.pop_front() {
            if visited.insert(id.clone()) {
                if let Some(next) = consumers.get(&id) {
                    pending.extend(next.iter().cloned());
                }
            }
        }
        Ok(visited)
    }
}

fn invalid(error: impl std::fmt::Display) -> DimensionEditError {
    DimensionEditError::InvalidDimension {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests;

fn strip_literal_dimensions(value: &mut Value) {
    match value {
        Value::Object(fields)
            if fields.len() == 2
                && matches!(
                    fields.get("kind").and_then(Value::as_str),
                    Some("literal" | "radius")
                )
                && fields.contains_key("value") =>
        {
            *value = fields.remove("value").unwrap_or(Value::Null);
            strip_literal_dimensions(value);
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                strip_literal_dimensions(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_literal_dimensions(value);
            }
        }
        _ => {}
    }
}

impl FeatureDefinition {
    /// Paths to dimensional operands in this standalone definition value.
    /// Expression tokens remain references inside their own canonical expression.
    pub fn dimension_paths(&self) -> Vec<String> {
        fn walk(value: &Value, path: &str, paths: &mut Vec<String>) {
            match value {
                Value::Object(fields)
                    if fields.len() == 2
                        && matches!(
                            fields.get("kind").and_then(Value::as_str),
                            Some("literal" | "parameter")
                        )
                        && fields.contains_key("value") =>
                {
                    paths.push(path.to_owned());
                }
                Value::Object(fields) => {
                    for (key, value) in fields {
                        if key != "tokens" {
                            let key = key.replace('~', "~0").replace('/', "~1");
                            walk(value, &format!("{path}/{key}"), paths);
                        }
                    }
                }
                Value::Array(values) => {
                    for (index, value) in values.iter().enumerate() {
                        walk(value, &format!("{path}/{index}"), paths);
                    }
                }
                _ => {}
            }
        }
        let mut paths = Vec::new();
        if let Ok(value) = serde_json::to_value(self) {
            walk(&value, "", &mut paths);
        }
        paths
    }

    /// Bind an operand while constructing a standalone definition from source.
    /// Editing a document uses `CadIr::set_feature_definition` to invalidate its graph.
    pub fn bind_dimension(
        &mut self,
        path: &str,
        parameter: ParameterId,
    ) -> Result<(), DimensionEditError> {
        if !self
            .dimension_paths()
            .iter()
            .any(|candidate| candidate == path)
        {
            return Err(DimensionEditError::InvalidPath {
                path: path.to_owned(),
            });
        }
        let mut value = serde_json::to_value(&*self).map_err(invalid)?;
        let selected = value
            .pointer_mut(path)
            .ok_or_else(|| DimensionEditError::InvalidPath {
                path: path.to_owned(),
            })?;
        *selected =
            serde_json::to_value(Dimension::<crate::scalar::FiniteReal>::Parameter(parameter))
                .map_err(invalid)?;
        *self = serde_json::from_value(value).map_err(invalid)?;
        Ok(())
    }
}

fn resolve_encoder_value<T: Clone + Serialize + DeserializeOwned + RewriteIdentities>(
    ir: &CadIr,
    definition: &T,
) -> Result<Value, DimensionResolutionError> {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(
        &arena,
        &DecodePolicy::default(),
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let resolved = definition.clone().resolve_dimensions(ir, &ctx)?;
    let mut value = serde_json::to_value(resolved).map_err(|error| {
        DimensionResolutionError::InvalidDefinition {
            reason: error.to_string(),
        }
    })?;
    // A binding can establish inequalities its id-only form could not decide.
    // discarded-value: Re-admission verifies cross-field constraints; the encoder consumes the same resolved JSON below.
    let _: T = serde_json::from_value(value.clone()).map_err(|error| {
        DimensionResolutionError::InvalidDefinition {
            reason: error.to_string(),
        }
    })?;
    strip_literal_dimensions(&mut value);
    Ok(value)
}
