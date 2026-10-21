// SPDX-License-Identifier: Apache-2.0
//! Neutral configuration tables and name-bound activation rules.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::DesignParameter;
use cadmpeg_ir::report::loss::LossNote;
use serde_json::{Map, Value};

use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};

pub(super) fn regenerate(
    target: &mut CadIr,
    native: &mut F3dNative,
    parameters: &[DesignParameter],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut ordered = target.model.configurations.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| (left.ordinal, &left.id).cmp(&(right.ordinal, &right.id)));
    let mut variants = Map::new();
    let mut order = Vec::new();
    let mut active = None;
    let mut rules = Vec::new();
    for configuration in ordered {
        let Some(name) = configuration
            .name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
        else {
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "configuration {} has no name and is omitted",
                configuration.id
            )));
            continue;
        };
        if variants.contains_key(name) {
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "configuration {} repeats the name {name} and is omitted",
                configuration.id
            )));
            continue;
        }
        let mut variant = Map::new();
        let mut overrides = Map::new();
        let mut suppressed = Vec::new();
        for (key, value) in &configuration.properties {
            if let Some(parameter) = key.as_str().strip_prefix("parameter:") {
                overrides.insert(parameter.to_owned(), Value::String(value.clone()));
            } else if let Some(feature) = key.as_str().strip_prefix("suppressed:") {
                if value == "true" {
                    suppressed.push(Value::String(feature.to_owned()));
                }
            } else if key.as_str().starts_with("activation_rule:") {
                rules.push((name.clone(), value.clone()));
            } else {
                losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                    "configuration {} property {key} is not encoded",
                    configuration.id
                )));
            }
        }
        for (id, expression) in &configuration.parameter_overrides {
            if let Some(parameter) = parameters.iter().find(|parameter| &parameter.id == id) {
                overrides.insert(
                    parameter.name.clone(),
                    Value::String(
                        expression
                            .render_with(|id| {
                                parameters
                                    .iter()
                                    .find(|candidate| &candidate.id == id)
                                    .map(|candidate| candidate.name.clone())
                            })
                            .ok_or_else(|| {
                                CodecError::InvalidInput(format!(
                                    "F3D configuration {} has a dangling expression reference",
                                    configuration.id
                                ))
                            })?,
                    ),
                );
            } else {
                // (C): a typed override must name a parameter held by the document.
                return Err(CodecError::malformed(format_args!(
                    "configuration {} references missing parameter {id}",
                    configuration.id
                )));
            }
        }
        for id in configuration.suppressed_features() {
            if !target
                .model
                .features
                .iter()
                .any(|feature| &feature.id == id)
            {
                return Err(CodecError::malformed(format_args!(
                    "configuration {} references missing feature {id}",
                    configuration.id,
                )));
            }
            // The generator cannot emit a neutral feature tree. A suppression
            // of one of those omitted records has no remaining native target.
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "configuration {} suppression of unsupported feature {id} is omitted",
                configuration.id,
            )));
        }
        if !configuration.parameter_values.is_empty()
            || configuration.bodies.is_some()
            || configuration.feature_states.values().any(|state| {
                !matches!(
                    state,
                    cadmpeg_ir::features::ConfigurationFeatureState::Suppressed {}
                )
            })
        {
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "configuration {} omits evaluated body/feature/parameter snapshots",
                configuration.id
            )));
        }
        if !overrides.is_empty() {
            variant.insert("parameters".into(), Value::Object(overrides));
        }
        if !suppressed.is_empty() {
            variant.insert("suppressed".into(), Value::Array(suppressed));
        }
        if let Some(material) = &configuration.material {
            variant.insert("material".into(), Value::String(material.clone()));
        }
        if configuration.active {
            if active.is_none() {
                active = Some(name.clone());
            } else {
                losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                    "configuration {} omits an additional active-state flag",
                    configuration.id
                )));
            }
        }
        variants.insert(name.clone(), Value::Object(variant));
        order.push(name.clone());
    }
    if !variants.is_empty() {
        let mut payload = Map::new();
        payload.insert("configurations".into(), Value::Object(variants));
        if let Some(active) = active {
            payload.insert("active".into(), Value::String(active));
        }
        native
            .design_configurations
            .push(DesignConfiguration::try_new(
                format!(
                    "{}/DesignConfigurationTable.generated.dsgcfg",
                    crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
                ),
                DesignConfigurationKind::Table,
                order,
                payload,
            )?);
    }
    for (ordinal, (activate, condition)) in rules.into_iter().enumerate() {
        let mut payload = Map::new();
        payload.insert("when".into(), Value::String(condition));
        payload.insert("activate".into(), Value::String(activate));
        native
            .design_configurations
            .push(DesignConfiguration::try_new(
                format!(
                    "{}/DesignConfigurationRule.generated-{ordinal}.dsgcfgrule",
                    crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
                ),
                DesignConfigurationKind::Rule,
                Vec::new(),
                payload,
            )?);
    }
    target.model.configurations =
        crate::design::configurations::project_configurations(&native.design_configurations)?;
    crate::design::configurations::bind_configuration_parameter_overrides(
        &mut target.model.configurations,
        &target.model.parameters,
    )?;
    crate::design::configurations::bind_configuration_suppressed_features(
        &mut target.model.configurations,
        &target.model.features,
    );
    crate::design::configurations::canonicalize_active_configuration(
        &mut target.model.configurations,
        &mut target.model.parameters,
        &mut target.model.features,
    )?;
    // A changed active expression cannot borrow the base cache. Re-author
    // parameters after canonicalization; the scalar writer can evaluate only
    // constant current expressions, and otherwise reports its usual omission.
    super::parameters::regenerate(target, native, losses)?;
    Ok(())
}
