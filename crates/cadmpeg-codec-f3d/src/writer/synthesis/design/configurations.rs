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
    let authored_features = if target
        .model
        .configurations
        .iter()
        .any(|configuration| configuration.suppressed_features().next().is_some())
    {
        super::super::super::generate::features::authored_feature_ids(target)
    } else {
        std::collections::BTreeSet::default()
    };
    let previous = std::mem::take(&mut native.design_configurations);
    let mut previous_tables = previous.iter().filter(|entry| {
        entry.kind() == DesignConfigurationKind::Table
            && entry
                .entry_name()
                .starts_with(crate::manifest::GENERATED_DESIGN_ASSET_FOLDER)
            && target.model.configurations.iter().any(|configuration| {
                configuration.native_ref.as_deref() == Some(entry.id().as_str())
            })
    });
    let previous_table = match (previous_tables.next(), previous_tables.next()) {
        (Some(table), None) => Some(table),
        _ => None,
    };
    let previous_payload = previous_table.map(DesignConfiguration::payload);
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
        let mut variant = previous_table
            .and_then(|table| {
                let variants = previous_payload
                    .as_ref()?
                    .get("configurations")?
                    .as_object()?;
                variants.iter().find_map(|(old_name, value)| {
                    (crate::ids::neutral_configuration_id(table.entry_name(), old_name)
                        == configuration.id)
                        .then(|| value.as_object().cloned())
                        .flatten()
                })
            })
            .unwrap_or_default();
        variant.remove("parameters");
        variant.remove("suppressed");
        variant.remove("material");
        let mut overrides = Map::new();
        let mut suppressed = Vec::new();
        for (key, value) in &configuration.properties {
            if let Some(parameter) = key.as_str().strip_prefix("parameter:") {
                overrides.insert(parameter.to_owned(), Value::String(value.clone()));
            } else if let Some(feature) = key.as_str().strip_prefix("suppressed:") {
                if value == "true" {
                    suppressed.push(Value::String(feature.to_owned()));
                }
            } else if let Some(source) = key.as_str().strip_prefix("activation_rule:") {
                rules.push((name.clone(), source.to_owned(), value.clone()));
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
            let Some(feature) = target
                .model
                .features
                .iter()
                .find(|feature| &feature.id == id)
            else {
                return Err(CodecError::malformed(format_args!(
                    "configuration {} references missing feature {id}",
                    configuration.id,
                )));
            };
            if !configuration.active && authored_features.contains(id) {
                if let Some(name) = feature.name.as_ref().filter(|name| {
                    target
                        .model
                        .features
                        .iter()
                        .filter(|candidate| candidate.name.as_ref() == Some(*name))
                        .count()
                        == 1
                }) {
                    if !suppressed.iter().any(|value| value.as_str() == Some(name)) {
                        suppressed.push(Value::String(name.clone()));
                    }
                    continue;
                }
            }
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
        let mut payload = previous_payload.unwrap_or_default();
        payload.remove("active");
        payload.insert("configurations".into(), Value::Object(variants));
        if let Some(active) = active {
            payload.insert("active".into(), Value::String(active));
        }
        let name = if let Some(table) = previous_table {
            table.entry_name().clone()
        } else {
            let mut suffix = 0usize;
            loop {
                let discriminator = if suffix == 0 {
                    String::new()
                } else {
                    format!("-{suffix}")
                };
                let candidate = format!(
                    "{}/DesignConfigurationTable.generated{discriminator}.dsgcfg",
                    crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
                );
                if !previous
                    .iter()
                    .any(|entry| entry.entry_name() == &candidate)
                {
                    break candidate;
                }
                suffix = suffix.checked_add(1).ok_or_else(|| {
                    CodecError::UnsupportedContent(
                        "F3D configuration table name space exhausted".into(),
                    )
                })?;
            }
        };
        native
            .design_configurations
            .push(DesignConfiguration::try_new(
                name,
                DesignConfigurationKind::Table,
                order,
                payload,
            )?);
    }
    let mut used_rule_names = std::collections::BTreeSet::new();
    let reserved_rule_names = previous
        .iter()
        .filter(|entry| entry.kind() == DesignConfigurationKind::Rule)
        .map(|entry| entry.entry_name().clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut next_rule_index = 0usize;
    for (activate, source, condition) in rules {
        let original = previous.iter().find(|entry| {
            entry.kind() == DesignConfigurationKind::Rule
                && entry.entry_name() == &source
                && !used_rule_names.contains(entry.entry_name())
                && entry
                    .entry_name()
                    .starts_with(crate::manifest::GENERATED_DESIGN_ASSET_FOLDER)
        });
        let mut payload = original.map_or_else(Map::new, DesignConfiguration::payload);
        payload.insert("when".into(), Value::String(condition));
        payload.insert("activate".into(), Value::String(activate));
        let name = if let Some(entry) = original {
            entry.entry_name().clone()
        } else {
            loop {
                let candidate = format!(
                    "{}/DesignConfigurationRule.generated-{next_rule_index}.dsgcfgrule",
                    crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
                );
                next_rule_index = next_rule_index.checked_add(1).ok_or_else(|| {
                    CodecError::UnsupportedContent(
                        "F3D configuration rule name space exhausted".into(),
                    )
                })?;
                if !reserved_rule_names.contains(&candidate)
                    && !used_rule_names.contains(&candidate)
                {
                    break candidate;
                }
            }
        };
        used_rule_names.insert(name.clone());
        native
            .design_configurations
            .push(DesignConfiguration::try_new(
                name,
                DesignConfigurationKind::Rule,
                Vec::new(),
                payload,
            )?);
    }
    for entry in &previous {
        if entry.kind() == DesignConfigurationKind::Table
            && entry.variants().is_empty()
            && entry.active().is_none()
            && entry
                .entry_name()
                .starts_with(crate::manifest::GENERATED_DESIGN_ASSET_FOLDER)
            && !native
                .design_configurations
                .iter()
                .any(|written| written.entry_name() == entry.entry_name())
        {
            native.design_configurations.push(entry.clone());
        }
    }
    for entry in &previous {
        if entry.kind() != DesignConfigurationKind::Rule
            || used_rule_names.contains(entry.entry_name())
            || !entry
                .entry_name()
                .starts_with(crate::manifest::GENERATED_DESIGN_ASSET_FOLDER)
        {
            continue;
        }
        let payload = entry.payload();
        let active_target = payload.get("activate").and_then(Value::as_str);
        let original_target_exists = active_target.is_some_and(|name| {
            previous
                .iter()
                .filter(|table| table.kind() == DesignConfigurationKind::Table)
                .flat_map(|table| table.variants().iter())
                .filter(|(candidate, _)| candidate == name)
                .count()
                == 1
        });
        let was_projected =
            original_target_exists && payload.get("when").and_then(Value::as_str).is_some();
        let target_resolves = active_target.is_none_or(|name| {
            target
                .model
                .configurations
                .iter()
                .filter(|configuration| configuration.name.as_deref() == Some(name))
                .count()
                == 1
        });
        if !was_projected && target_resolves {
            native.design_configurations.push(entry.clone());
        }
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
    Ok(())
}
