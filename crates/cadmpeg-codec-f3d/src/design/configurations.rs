// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::cloned_ref_to_slice_refs))]
//! Decode and project Design configuration records.

use crate::container::{role, ContainerScan};
use crate::design::dimensions::json_scalar_text;
use crate::ids::{self, neutral_configuration_id};
use crate::records::{DesignConfiguration, DesignConfigurationKind};
use cadmpeg_codec_core::CodecError;
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
        // A rule document is an open JSON object. Only the closed, typed
        // projection with both `when` and `activate` as strings has neutral
        // activation semantics; every other shape remains native JSON and is
        // deliberately not interpreted as a partial rule.
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
                bodies: cadmpeg_ir::features::ConfigurationBodies::Unresolved,
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

#[cfg(test)]
mod tests {
    use super::{
        bind_configuration_parameter_overrides, bind_configuration_suppressed_features,
        project_configurations, unresolved_configuration_member_count,
        unresolved_configuration_parameter_override_count, unresolved_configuration_rule_count,
        unresolved_configuration_suppressed_feature_count, validate_configuration_payload,
    };
    use crate::records::{DesignConfiguration, DesignConfigurationKind};
    use cadmpeg_ir::features::{
        DesignParameter as NeutralParameter, Feature, FeatureDefinition, FeatureId, ParameterId,
    };
    use std::collections::{BTreeMap, HashSet};

    #[test]
    fn configuration_identity_is_stable_across_table_order_and_delimiter_names() {
        let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
            id: format!("f3d:configuration:entry#{entry_name}"),
            entry_name: entry_name.into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({"configurations": {variant_name: {}}}),
        };
        let first = table("asset/a#b.dsgcfg", "c");
        let second = table("asset/a.dsgcfg", "b#c");
        let first_id = first.id.clone();

        let forward = project_configurations(&[first.clone(), second.clone()]);
        let reversed = project_configurations(&[second, first]);
        let forward_ids = forward
            .iter()
            .map(|configuration| configuration.id.clone())
            .collect::<HashSet<_>>();
        let reversed_ids = reversed
            .iter()
            .map(|configuration| configuration.id.clone())
            .collect::<HashSet<_>>();

        assert_eq!(forward_ids, reversed_ids);
        assert_eq!(forward_ids.len(), 2);
        assert_ne!(forward[0].id, forward[1].id);
        assert_eq!(forward[0].native_ref.as_deref(), Some(first_id.as_str()));
    }

    #[test]
    fn configuration_parameter_overrides_require_scalar_values() {
        let scalar_parameters = serde_json::json!({
            "configurations": {
                "variant": {
                    "parameters": {
                        "string": "25 mm",
                        "number": 2.5,
                        "boolean": true,
                        "null": null
                    }
                }
            }
        });
        assert!(validate_configuration_payload(
            "table.dsgcfg",
            DesignConfigurationKind::Table,
            &scalar_parameters,
        )
        .is_ok());

        for value in [
            serde_json::json!(["25 mm"]),
            serde_json::json!({"value": "25 mm"}),
        ] {
            let payload = serde_json::json!({
                "configurations": {"variant": {"parameters": {"width": value}}}
            });
            assert!(validate_configuration_payload(
                "table.dsgcfg",
                DesignConfigurationKind::Table,
                &payload,
            )
            .is_err());
        }
    }

    #[test]
    fn configuration_unknown_members_are_counted_at_each_semantic_level() {
        let native = [
            DesignConfiguration {
                id: "f3d:configuration:entry#table.dsgcfg".into(),
                entry_name: "table.dsgcfg".into(),
                kind: DesignConfigurationKind::Table,
                payload: serde_json::json!({
                    "active": "variant",
                    "table_unknown": 1,
                    "configurations": {
                        "variant": {
                            "parameters": {},
                            "suppressed": [],
                            "material": "steel",
                            "variant_unknown": true
                        }
                    }
                }),
            },
            DesignConfiguration {
                id: "f3d:configuration:entry#rule.dsgcfgrule".into(),
                entry_name: "rule.dsgcfgrule".into(),
                kind: DesignConfigurationKind::Rule,
                payload: serde_json::json!({
                    "when": "width > 20 mm",
                    "activate": "variant",
                    "rule_unknown": null
                }),
            },
        ];
        assert_eq!(unresolved_configuration_member_count(&native), 3);
    }

    #[test]
    fn configuration_rule_without_the_typed_pair_is_retained_not_rejected() {
        let native = [DesignConfiguration {
            id: "f3d:configuration:entry#partial.dsgcfgrule".into(),
            entry_name: "partial.dsgcfgrule".into(),
            kind: DesignConfigurationKind::Rule,
            payload: serde_json::json!({"when": "width > 20 mm", "vendorExtension": 7}),
        }];
        assert!(validate_configuration_payload(
            "partial.dsgcfgrule",
            DesignConfigurationKind::Rule,
            &native[0].payload,
        )
        .is_ok());
        let projected = project_configurations(&native);
        assert!(projected.is_empty());
        assert_eq!(unresolved_configuration_rule_count(&native, &projected), 1);
    }

    #[test]
    fn configuration_rules_bind_only_one_named_variant() {
        let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
            id: format!("f3d:configuration:entry#{entry_name}"),
            entry_name: entry_name.into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({"configurations": {variant_name: {}}}),
        };
        let rule = DesignConfiguration {
            id: "f3d:configuration:entry#rule.dsgcfgrule".into(),
            entry_name: "rule.dsgcfgrule".into(),
            kind: DesignConfigurationKind::Rule,
            payload: serde_json::json!({"when": "width > 20 mm", "activate": "wide"}),
        };
        let native = [table("table.dsgcfg", "wide"), rule.clone()];
        let projected = project_configurations(&native);
        assert_eq!(
            projected[0].properties["activation_rule:rule.dsgcfgrule"],
            "width > 20 mm"
        );
        assert_eq!(unresolved_configuration_rule_count(&native, &projected), 0);

        let ambiguous = [
            table("first.dsgcfg", "wide"),
            table("second.dsgcfg", "wide"),
            rule,
        ];
        let projected = project_configurations(&ambiguous);
        assert!(projected
            .iter()
            .all(|configuration| configuration.properties.is_empty()));
        assert_eq!(
            unresolved_configuration_rule_count(&ambiguous, &projected),
            1
        );
    }

    #[test]
    fn configuration_parameter_overrides_bind_only_unique_parameter_names() {
        let table = DesignConfiguration {
            id: "f3d:configuration:entry#table.dsgcfg".into(),
            entry_name: "table.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({
                "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
            }),
        };
        let parameter = NeutralParameter {
            id: ParameterId("f3d:model:parameter#width".into()),
            owner: None,
            ordinal: 0,
            name: "width".into(),
            expression: "10 mm".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let mut projected = project_configurations(&[table]);
        bind_configuration_parameter_overrides(&mut projected, std::slice::from_ref(&parameter));
        assert_eq!(projected[0].parameter_overrides[&parameter.id], "25 mm");
        assert!(projected[0].properties.is_empty());
        assert_eq!(
            unresolved_configuration_parameter_override_count(&projected),
            0
        );

        let duplicate = NeutralParameter {
            id: ParameterId("f3d:model:parameter#other-width".into()),
            ..parameter.clone()
        };
        let mut ambiguous = project_configurations(&[DesignConfiguration {
            id: "f3d:configuration:entry#other.dsgcfg".into(),
            entry_name: "other.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({
                "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
            }),
        }]);
        bind_configuration_parameter_overrides(&mut ambiguous, &[parameter, duplicate]);
        assert!(ambiguous[0].parameter_overrides.is_empty());
        assert_eq!(
            unresolved_configuration_parameter_override_count(&ambiguous),
            1
        );
    }

    #[test]
    fn configuration_suppression_binds_only_unique_feature_names() {
        let table = DesignConfiguration {
            id: "f3d:configuration:entry#table.dsgcfg".into(),
            entry_name: "table.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({
                "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
            }),
        };
        let feature = Feature {
            id: FeatureId("f3d:model:feature#fillet-1".into()),
            ordinal: 0,
            name: Some("Fillet 1".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "Fillet".into(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
            native_ref: None,
        };
        let mut projected = project_configurations(&[table]);
        bind_configuration_suppressed_features(&mut projected, std::slice::from_ref(&feature));
        assert_eq!(projected[0].suppressed_features, [feature.id.clone()]);
        assert!(projected[0].properties.is_empty());
        assert_eq!(
            unresolved_configuration_suppressed_feature_count(&projected),
            0
        );

        let duplicate = Feature {
            id: FeatureId("f3d:model:feature#other-fillet-1".into()),
            ..feature.clone()
        };
        let mut ambiguous = project_configurations(&[DesignConfiguration {
            id: "f3d:configuration:entry#other.dsgcfg".into(),
            entry_name: "other.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            payload: serde_json::json!({
                "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
            }),
        }]);
        bind_configuration_suppressed_features(&mut ambiguous, &[feature, duplicate]);
        assert!(ambiguous[0].suppressed_features.is_empty());
        assert_eq!(
            unresolved_configuration_suppressed_feature_count(&ambiguous),
            1
        );
    }
}
