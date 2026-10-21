// SPDX-License-Identifier: Apache-2.0
//! Canonical dimensional bindings, expression caches and dependency cycles.

use std::collections::BTreeMap;

use cadmpeg_core::{decode::DecodeContext, CodecError};
use serde::{de::DeserializeOwned, Serialize};

use crate::features::DimensionResolutionError;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::schema::rewrite::typed::RewriteIdentities;
use crate::CadIr;

pub(super) fn check(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for parameter in &ir.model.parameters {
        if let Some(value) = &parameter.value {
            if let Some(literal) = parameter.expression.literal_value_as(value) {
                let agrees = match (&literal, value) {
                    (
                        crate::features::ParameterValue::String(left),
                        crate::features::ParameterValue::String(right),
                    ) => left == right,
                    _ => crate::compare::values_agree(
                        &serde_json::to_value(&literal)
                            .map_err(|error| CodecError::malformed(error.to_string()))?,
                        &serde_json::to_value(value)
                            .map_err(|error| CodecError::malformed(error.to_string()))?,
                    )
                    .is_ok(),
                };
                let stale = parameter.owner.as_ref().is_some_and(|owner| {
                    ir.model
                        .features
                        .iter()
                        .any(|feature| feature.id == *owner && feature.needs_recompute)
                });
                if !agrees && !stale {
                    super::record_finding(findings, Check::GeometricConsistency, Severity::Error, Some(parameter.id.as_str()),
                        format_args!("parameter cached value disagrees with its canonical literal expression"));
                }
            }
        }
        if let Some(pmi) = &parameter.pmi {
            if !ir.model.pmi.iter().any(|annotation| annotation.id == *pmi) {
                super::record_related_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    parameter.id.as_str(),
                    &[pmi.as_str()],
                    format_args!("parameter PMI reference does not resolve"),
                );
            }
        }
    }
    for feature in &ir.model.features {
        check_record(ctx, ir, feature.id.as_str(), feature.definition(), findings)?;
        if matches!(
            feature.definition().operation(),
            crate::features::FeatureOperation::AssemblyJoint { .. }
        ) && feature.suppressed.is_some()
        {
            super::record_finding(findings,Check::ReferentialIntegrity,Severity::Error,
                Some(feature.id.as_str()),format_args!("assembly-joint suppression belongs only to the referenced joint; its feature must omit suppressed"));
        }
        if let crate::features::FeatureOperation::TreeNode { children, .. } =
            feature.definition().operation()
        {
            let content_children: Vec<_> = feature
                .source_content
                .iter()
                .filter_map(|item| match item {
                    crate::features::FeatureSourceContent::Feature(child) => Some(child),
                    _ => None,
                })
                .collect();
            if !content_children.is_empty() {
                let independent_ordering = feature
                    .source_content
                    .iter()
                    .any(|item| !matches!(item, crate::features::FeatureSourceContent::Feature(_)));
                if content_children != children.iter().collect::<Vec<_>>() || !independent_ordering
                {
                    super::record_finding(findings,Check::ReferentialIntegrity,Severity::Error,
                        Some(feature.id.as_str()),format_args!("source-content child ordering must agree with canonical tree children and add independent text or parameter ordering"));
                }
            }
        }
        let law = match feature.definition().operation() {
            crate::features::FeatureOperation::Helix { shape, .. } => shape.law(),
            crate::features::FeatureOperation::HelixNativeAxis { law, .. } => Some(law),
            _ => None,
        };
        if let Some(crate::features::HelixLaw::HeightPitch { height, pitch }) = law {
            if ir
                .dimension_value(height)
                .zip(ir.dimension_value(pitch))
                .is_some_and(|(height, pitch)| {
                    height.get() == 0.0
                        || height.get().is_sign_negative() != pitch.get().is_sign_negative()
                })
            {
                super::record_finding(
                    findings,
                    Check::GeometricConsistency,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!(
                        "helix authored height and pitch do not produce a positive turn count"
                    ),
                );
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        check_record(
            ctx,
            ir,
            constraint.id.as_str(),
            &constraint.definition,
            findings,
        )?;
        if constraint.driving.is_some()
            && !constraint
                .definition
                .kind()
                .dimension_parameters()
                .map_err(|error| CodecError::malformed(error.to_string()))?
                .is_empty()
        {
            super::record_finding(findings, Check::ReferentialIntegrity, Severity::Error, Some(constraint.id.as_str()),
                format_args!("bound sketch dimensions use the canonical parameter's driving state; constraint driving is only for literals"));
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        check_record(
            ctx,
            ir,
            constraint.id.as_str(),
            &constraint.definition,
            findings,
        )?;
    }
    for annotation in &ir.model.pmi {
        check_record(
            ctx,
            ir,
            annotation.id.as_str(),
            &annotation.definition,
            findings,
        )?;
    }
    for configuration in &ir.model.configurations {
        if !configuration.active {
            check_configuration_differences(ir, configuration, findings);
            config_bindings::check(ctx, ir, configuration, findings)?;
        }
        if !configuration.feature_states.is_empty() {
            let identities: Vec<_> = ir
                .model
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect();
            let positions: BTreeMap<_, _> = identities
                .iter()
                .enumerate()
                .map(|(index, id)| (*id, index))
                .collect();
            let dependencies: Vec<Vec<_>> = ir
                .model
                .features
                .iter()
                .map(|feature| {
                    let references = configuration
                        .feature_states
                        .get(&feature.id)
                        .and_then(crate::features::ConfigurationFeatureState::definition)
                        .map_or_else(
                            || feature.dependencies.iter().collect(),
                            |definition| {
                                super::topology::regeneration_references(definition.operation())
                            },
                        );
                    references
                        .into_iter()
                        .filter_map(|id| positions.get(id.as_str()).copied())
                        .collect()
                })
                .collect();
            check_dependency_cycles(
                ctx,
                &identities,
                &dependencies,
                "feature",
                Some(configuration.id.as_str()),
                findings,
            )?;
        }
        // Typed dimension resolution reads only parameters. This temporary
        // context contains their current configuration state, without cloning
        // any model/native payload or retaining another authored value.
        let mut context = CadIr::empty();
        context.model.parameters = ir
            .model
            .parameters
            .iter()
            .map(|parameter| {
                let mut resolved = parameter.clone();
                resolved.expression = configuration
                    .parameter_expression(ir, &parameter.id)
                    .cloned()
                    .unwrap_or_else(|| parameter.expression.clone());
                resolved.value = configuration.parameter_value(ir, &parameter.id).cloned();
                resolved
            })
            .collect();
        if !configuration.parameter_overrides.is_empty() {
            check_parameter_cycles(ctx, &context, Some(configuration.id.as_str()), findings)?;
        }
        for feature in &ir.model.features {
            let state = configuration.feature_states.get(&feature.id);
            let suppressed = state.map_or(
                feature.suppressed == Some(true),
                crate::features::ConfigurationFeatureState::is_suppressed,
            );
            if suppressed {
                continue;
            }
            let definition = state
                .and_then(crate::features::ConfigurationFeatureState::definition)
                .unwrap_or_else(|| feature.definition());
            check_record(
                ctx,
                &context,
                configuration.id.as_str(),
                definition,
                findings,
            )?;
        }
        for (id, value) in &configuration.parameter_values {
            let parameter = ir
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *id);
            let expression = configuration
                .parameter_overrides
                .get(id)
                .or_else(|| parameter.map(|parameter| &parameter.expression));
            if let Some(literal) =
                expression.and_then(|expression| expression.literal_value_as(value))
            {
                let agrees = match (&literal, value) {
                    (
                        crate::features::ParameterValue::String(first),
                        crate::features::ParameterValue::String(second),
                    ) => first == second,
                    _ => crate::compare::values_agree(
                        &serde_json::to_value(&literal)
                            .map_err(|error| CodecError::malformed(error.to_string()))?,
                        &serde_json::to_value(value)
                            .map_err(|error| CodecError::malformed(error.to_string()))?,
                    )
                    .is_ok(),
                };
                let stale = parameter
                    .and_then(|parameter| parameter.owner.as_ref())
                    .is_some_and(|owner| {
                        ir.model
                            .features
                            .iter()
                            .any(|feature| feature.id == *owner && feature.needs_recompute)
                    });
                if !agrees && !stale {
                    super::record_related_finding(findings, Check::GeometricConsistency, Severity::Error,
                        configuration.id.as_str(), &[id.as_str()], format_args!("configuration parameter cache disagrees with its canonical literal expression"));
                }
            }
        }
    }
    check_parameter_cycles(ctx, ir, None, findings)?;
    let identities: Vec<_> = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect();
    let positions: BTreeMap<_, _> = identities
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let dependencies = ir
        .model
        .features
        .iter()
        .map(|feature| {
            feature
                .dependencies
                .iter()
                .filter_map(|id| positions.get(id.as_str()).copied())
                .collect()
        })
        .collect::<Vec<Vec<_>>>();
    check_dependency_cycles(ctx, &identities, &dependencies, "feature", None, findings)
}

fn check_configuration_differences(
    ir: &CadIr,
    configuration: &crate::features::DesignConfiguration,
    findings: &mut Vec<Finding>,
) {
    for (id, expression) in &configuration.parameter_overrides {
        if ir
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.id == *id && parameter.expression == *expression)
        {
            super::record_related_finding(findings, Check::ReferentialIntegrity, Severity::Error,
                configuration.id.as_str(), &[id.as_str()],
                format_args!("inactive configuration repeats the base parameter expression instead of storing a difference"));
        }
    }
    for (id, value) in &configuration.parameter_values {
        if !configuration.parameter_overrides.contains_key(id)
            && ir
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.id == *id && parameter.value.as_ref() == Some(value))
        {
            super::record_related_finding(findings, Check::ReferentialIntegrity, Severity::Error,
                configuration.id.as_str(), &[id.as_str()],
                format_args!("inactive configuration repeats the base parameter cache instead of storing a difference"));
        }
    }
    for (id, state) in &configuration.feature_states {
        let Some(feature) = ir.model.features.iter().find(|feature| feature.id == *id) else {
            continue;
        };
        let redundant = match state {
            crate::features::ConfigurationFeatureState::Suppressed {} => {
                feature.suppressed == Some(true)
            }
            crate::features::ConfigurationFeatureState::Active {} => {
                feature.suppressed != Some(true)
            }
            crate::features::ConfigurationFeatureState::Definition { definition } => {
                definition == feature.definition()
            }
        };
        if redundant {
            super::record_related_finding(findings, Check::ReferentialIntegrity, Severity::Error,
                configuration.id.as_str(), &[id.as_str()],
                format_args!("inactive configuration repeats the base feature state instead of storing a difference"));
        }
    }
}

fn check_record<T: RewriteIdentities + Clone + Serialize + DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    id: &str,
    record: &T,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut errors = Vec::new();
    record.validate_dimensions(ir, ctx, &mut errors)?;
    for error in errors {
        super::record_finding(
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            Some(id),
            format_args!("invalid canonical dimension: {error}"),
        );
    }
    match record.clone().resolve_dimensions(ir, ctx) {
        Ok(resolved) => {
            let value = serde_json::to_value(resolved)
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            if let Err(error) = serde_json::from_value::<T>(value) {
                super::record_finding(findings, Check::GeometricConsistency, Severity::Error, Some(id),
                    format_args!("resolved dimensions violate the definition's quantity or relation: {error}"));
            }
        }
        Err(DimensionResolutionError::ResourceLimit(limit)) => {
            return Err(CodecError::ResourceLimit(limit))
        }
        Err(DimensionResolutionError::InvalidDefinition { reason }) => {
            super::record_finding(
                findings,
                Check::GeometricConsistency,
                Severity::Error,
                Some(id),
                format_args!("resolved dimensions cannot evaluate their definition: {reason}"),
            );
        }
        // Unevaluated parameters are legal. Reference/quantity errors were all
        // collected above, rather than stopping at the first unresolved slot.
        Err(_) => {}
    }
    Ok(())
}

fn check_parameter_cycles(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    configuration: Option<&str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let positions: BTreeMap<_, _> = ir
        .model
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| (parameter.id.as_str(), index))
        .collect();
    let dependencies: Vec<Vec<_>> = ir
        .model
        .parameters
        .iter()
        .map(|parameter| {
            parameter
                .expression
                .dependencies()
                .filter_map(|id| positions.get(id.as_str()).copied())
                .collect()
        })
        .collect();
    let identities: Vec<_> = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect();
    check_dependency_cycles(
        ctx,
        &identities,
        &dependencies,
        "parameter expression",
        configuration,
        findings,
    )
}

fn check_dependency_cycles(
    ctx: &DecodeContext<'_>,
    identities: &[&str],
    dependencies: &[Vec<usize>],
    kind: &str,
    configuration: Option<&str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut visits = vec![0_u8; dependencies.len()];
    for start in 0..dependencies.len() {
        if visits[start] != 0 {
            continue;
        }
        visits[start] = 1;
        let mut stack = vec![(
            start,
            0_usize,
            ctx.enter_nested("design dependency cycle traversal")?,
        )];
        while let Some((node, child_index, _)) = stack.last_mut() {
            let Some(child) = dependencies[*node].get(*child_index).copied() else {
                visits[*node] = 2;
                stack.pop();
                continue;
            };
            *child_index += 1;
            if visits[child] == 1 {
                let related: Vec<_> = stack
                    .iter()
                    .skip_while(|(node, _, _)| *node != child)
                    .map(|(node, _, _)| identities[*node])
                    .collect();
                super::record_related_finding(findings, Check::ReferentialIntegrity, Severity::Warning,
                    configuration.unwrap_or_else(|| identities[child]), &related, format_args!("{kind} dependency cycle is retained{}; evaluation requires a cycle-aware solver", if configuration.is_some() { " in this configuration" } else { "" }));
            } else if visits[child] == 0 {
                visits[child] = 1;
                stack.push((
                    child,
                    0,
                    ctx.enter_nested("design dependency cycle traversal")?,
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

mod config_bindings;
