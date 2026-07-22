// SPDX-License-Identifier: Apache-2.0
//! Decode and project Design configuration records.

use crate::container::{role, ContainerScan};
use crate::design::dimensions::json_scalar_text;
use crate::ids::{self, neutral_configuration_id};
use crate::records::{DesignConfiguration, DesignConfigurationKind};
use cadmpeg_ir::codec::CodecError;
use std::collections::HashSet;

/// Decode every JSON design-configuration table and rule entry.
pub fn decode_configurations(scan: &ContainerScan) -> Result<Vec<DesignConfiguration>, CodecError> {
    let configurations = scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::DESIGN_CONFIG)
        .map(|entry| {
            let bytes = scan.entry_bytes(&entry.name)?;
            let payload: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
                CodecError::Malformed(format!(
                    "invalid F3D configuration JSON {}: {error}",
                    entry.name
                ))
            })?;
            if !payload.is_object() {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration JSON must be an object: {}",
                    entry.name
                )));
            }
            let kind = if entry.name.ends_with(".dsgcfgrule") {
                DesignConfigurationKind::Rule
            } else {
                DesignConfigurationKind::Table
            };
            validate_configuration_payload(&entry.name, kind, &payload)?;
            Ok(DesignConfiguration {
                id: ids::configuration_entry_id(&entry.name),
                entry_name: entry.name.clone(),
                kind,
                payload,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    for configuration in &configurations {
        if !names.insert(configuration.entry_name.as_str())
            || !ids.insert(configuration.id.as_str())
        {
            return Err(CodecError::Malformed(format!(
                "duplicate F3D configuration identity: {}",
                configuration.entry_name
            )));
        }
    }
    Ok(configurations)
}

/// Validate the typed fields of one configuration document while permitting
/// unrecognized object members for forward-compatible native retention.
pub(crate) fn validate_configuration_payload(
    entry_name: &str,
    kind: DesignConfigurationKind,
    payload: &serde_json::Value,
) -> Result<(), CodecError> {
    let object = payload.as_object().ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D configuration JSON must be an object: {entry_name}"
        ))
    })?;
    if kind == DesignConfigurationKind::Rule {
        let condition = object.get("when");
        let target = object.get("activate");
        if (condition.is_some() || target.is_some())
            && (!condition.is_some_and(serde_json::Value::is_string)
                || !target.is_some_and(serde_json::Value::is_string))
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration rule `when` and `activate` must be paired strings: {entry_name}"
            )));
        }
        return Ok(());
    }
    let configurations = match object.get("configurations") {
        Some(value) => Some(value.as_object().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration table `configurations` must be an object: {entry_name}"
            ))
        })?),
        None => None,
    };
    if let Some(active) = object.get("active") {
        let active = active.as_str().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration table `active` must be a string: {entry_name}"
            ))
        })?;
        if !configurations.is_some_and(|variants| variants.contains_key(active)) {
            return Err(CodecError::Malformed(format!(
                "F3D active configuration `{active}` is not a named variant: {entry_name}"
            )));
        }
    }
    for (name, value) in configurations.into_iter().flatten() {
        let definition = value.as_object().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration variant `{name}` must be an object: {entry_name}"
            ))
        })?;
        if definition
            .get("parameters")
            .is_some_and(|value| !value.is_object())
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` parameters must be an object: {entry_name}"
            )));
        }
        if definition
            .get("parameters")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|parameters| {
                parameters
                    .values()
                    .any(|value| value.is_array() || value.is_object())
            })
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` parameter overrides must be JSON scalars: {entry_name}"
            )));
        }
        if let Some(suppressed) = definition.get("suppressed") {
            let valid = suppressed
                .as_array()
                .is_some_and(|values| values.iter().all(serde_json::Value::is_string));
            if !valid {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration variant `{name}` suppressed list must contain strings: {entry_name}"
                )));
            }
        }
        if definition
            .get("material")
            .is_some_and(|value| !value.is_string())
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` material must be a string: {entry_name}"
            )));
        }
    }
    Ok(())
}

/// Project named variants from configuration-table JSON into the neutral
/// configuration arena. Rule documents remain in the native arena because a
/// rule is a selector, not a model variant.
pub fn project_configurations(
    native: &[DesignConfiguration],
) -> Vec<cadmpeg_ir::features::DesignConfiguration> {
    use cadmpeg_ir::features::DesignConfiguration as NeutralConfiguration;
    use std::collections::BTreeMap;

    let mut projected = Vec::new();
    for table in native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Table)
    {
        let active = table
            .payload
            .get("active")
            .and_then(serde_json::Value::as_str);
        let Some(configurations) = table
            .payload
            .get("configurations")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (name, definition) in configurations {
            let mut properties = BTreeMap::new();
            let definition = definition.as_object();
            if let Some(parameters) = definition
                .and_then(|value| value.get("parameters"))
                .and_then(serde_json::Value::as_object)
            {
                for (parameter, value) in parameters {
                    properties.insert(format!("parameter:{parameter}"), json_scalar_text(value));
                }
            }
            if let Some(suppressed) = definition
                .and_then(|value| value.get("suppressed"))
                .and_then(serde_json::Value::as_array)
            {
                for feature in suppressed.iter().filter_map(serde_json::Value::as_str) {
                    properties.insert(format!("suppressed:{feature}"), "true".into());
                }
            }
            let material = definition
                .and_then(|value| value.get("material"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let ordinal = u32::try_from(projected.len()).unwrap_or(u32::MAX);
            projected.push(NeutralConfiguration {
                id: neutral_configuration_id(&table.entry_name, name),
                ordinal,
                active: active == Some(name.as_str()),
                source_index: None,
                name: name.clone(),
                material,
                properties,
                parameter_overrides: BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                bodies: Vec::new(),
                native_ref: Some(table.id.clone()),
            });
        }
    }
    for rule in native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Rule)
    {
        let Some(condition) = rule.payload.get("when").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = rule
            .payload
            .get("activate")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let mut matches = projected
            .iter_mut()
            .filter(|configuration| configuration.name == target);
        let Some(configuration) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        configuration.properties.insert(
            format!("activation_rule:{}", rule.entry_name),
            condition.to_owned(),
        );
    }
    projected
}

/// Replace name-keyed configuration properties with stable parameter references
/// when exactly one neutral parameter has the named source identity.
pub fn bind_configuration_parameter_overrides(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) {
    for configuration in configurations {
        let override_names = configuration
            .properties
            .keys()
            .filter_map(|key| key.strip_prefix("parameter:"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for name in override_names {
            let mut matches = parameters.iter().filter(|parameter| parameter.name == name);
            let Some(parameter) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let key = format!("parameter:{name}");
            let expression = configuration
                .properties
                .remove(&key)
                .expect("configuration override key came from this map");
            configuration
                .parameter_overrides
                .insert(parameter.id.clone(), expression);
        }
    }
}

/// Replace name-keyed suppression properties with stable feature references
/// when exactly one neutral feature has the named source identity.
pub fn bind_configuration_suppressed_features(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    features: &[cadmpeg_ir::features::Feature],
) {
    for configuration in configurations {
        let names = configuration
            .properties
            .keys()
            .filter_map(|key| key.strip_prefix("suppressed:"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for name in names {
            let mut matches = features
                .iter()
                .filter(|feature| feature.name.as_deref() == Some(name.as_str()));
            let Some(feature) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            configuration
                .properties
                .remove(&format!("suppressed:{name}"));
            configuration.suppressed_features.push(feature.id.clone());
        }
    }
}

pub(crate) fn unresolved_configuration_parameter_override_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.starts_with("parameter:"))
        .count()
}

pub(crate) fn unresolved_configuration_suppressed_feature_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.starts_with("suppressed:"))
        .count()
}

pub(crate) fn unresolved_configuration_rule_count(
    native: &[DesignConfiguration],
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    native
        .iter()
        .filter(|rule| {
            rule.kind == DesignConfigurationKind::Rule
                && rule
                    .payload
                    .as_object()
                    .is_some_and(|object| !object.is_empty())
        })
        .filter(|rule| {
            !projected.iter().any(|configuration| {
                configuration
                    .properties
                    .contains_key(&format!("activation_rule:{}", rule.entry_name))
            })
        })
        .count()
}

pub(crate) fn unresolved_configuration_member_count(native: &[DesignConfiguration]) -> usize {
    native
        .iter()
        .map(|configuration| {
            let Some(object) = configuration.payload.as_object() else {
                return 0;
            };
            match configuration.kind {
                DesignConfigurationKind::Rule => object
                    .keys()
                    .filter(|key| !matches!(key.as_str(), "when" | "activate"))
                    .count(),
                DesignConfigurationKind::Table => {
                    let table_members = object
                        .keys()
                        .filter(|key| !matches!(key.as_str(), "configurations" | "active"))
                        .count();
                    let variant_members = object
                        .get("configurations")
                        .and_then(serde_json::Value::as_object)
                        .into_iter()
                        .flat_map(|variants| variants.values())
                        .filter_map(serde_json::Value::as_object)
                        .flat_map(|variant| variant.keys())
                        .filter(|key| {
                            !matches!(key.as_str(), "parameters" | "suppressed" | "material")
                        })
                        .count();
                    table_members + variant_members
                }
            }
        })
        .sum()
}
