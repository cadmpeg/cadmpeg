// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::cloned_ref_to_slice_refs))]
//! Decode and project Design configuration records.

use crate::container::{role, ContainerScan};
use crate::design::dimensions::json_scalar_text;
use crate::ids::{self, neutral_configuration_id};
use crate::records::{DesignConfiguration, DesignConfigurationKind};
use cadmpeg_core::CodecError;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
use std::fmt;

#[derive(Default)]
struct OrderedVariantNames(Vec<String>);

impl<'de> Deserialize<'de> for OrderedVariantNames {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedVariantNamesVisitor;

        impl<'de> Visitor<'de> for OrderedVariantNamesVisitor {
            type Value = OrderedVariantNames;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a configuration-variant object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut names = Vec::new();
                let mut unique = HashSet::new();
                while let Some(name) = map.next_key::<String>()? {
                    if !unique.insert(name.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate configuration variant {name:?}"
                        )));
                    }
                    map.next_value::<IgnoredAny>()?;
                    names.push(name);
                }
                Ok(OrderedVariantNames(names))
            }
        }

        deserializer.deserialize_map(OrderedVariantNamesVisitor)
    }
}

#[derive(Deserialize)]
struct ConfigurationMemberOrder {
    #[serde(default)]
    configurations: OrderedVariantNames,
}

fn parse_configuration_variant_order(
    entry_name: &str,
    bytes: &[u8],
) -> Result<Vec<String>, CodecError> {
    serde_json::from_slice::<ConfigurationMemberOrder>(bytes)
        .map(|order| order.configurations.0)
        .map_err(|error| {
            CodecError::malformed(format_args!(
                "invalid F3D configuration variant order {entry_name}: {error}"
            ))
        })
}

/// Decode every JSON design-configuration table and rule entry.
pub fn decode_configurations(scan: &ContainerScan) -> Result<Vec<DesignConfiguration>, CodecError> {
    let configurations = scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, role::DESIGN_CONFIG))
        .map(|entry| {
            let bytes = scan.entry_bytes(&entry.name)?;
            let payload: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
                CodecError::malformed(format_args!(
                    "invalid F3D configuration JSON {}: {error}",
                    entry.name
                ))
            })?;
            if !payload.is_object() {
                return Err(CodecError::malformed(format_args!(
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
            let variant_order = if kind == DesignConfigurationKind::Table {
                parse_configuration_variant_order(&entry.name, bytes)?
            } else {
                Vec::new()
            };
            Ok(DesignConfiguration {
                id: ids::configuration_entry_id(&entry.name),
                entry_name: entry.name.clone(),
                kind,
                variant_order,
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
            return Err(CodecError::malformed(format_args!(
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
        CodecError::malformed(format_args!(
            "F3D configuration JSON must be an object: {entry_name}"
        ))
    })?;
    if kind == DesignConfigurationKind::Rule {
        // Rules need typed `when` and `activate` strings; other shapes stay native JSON.
        return Ok(());
    }
    let configurations = match object.get("configurations") {
        Some(value) => Some(value.as_object().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D configuration table `configurations` must be an object: {entry_name}"
            ))
        })?),
        None => None,
    };
    if let Some(active) = object.get("active") {
        let active = active.as_str().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D configuration table `active` must be a string: {entry_name}"
            ))
        })?;
        if !configurations.is_some_and(|variants| variants.contains_key(active)) {
            return Err(CodecError::malformed(format_args!(
                "F3D active configuration `{active}` is not a named variant: {entry_name}"
            )));
        }
    }
    for (name, value) in configurations.into_iter().flatten() {
        let definition = value.as_object().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D configuration variant `{name}` must be an object: {entry_name}"
            ))
        })?;
        if definition
            .get("parameters")
            .is_some_and(|value| !value.is_object())
        {
            return Err(CodecError::malformed(format_args!(
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
            return Err(CodecError::malformed(format_args!(
                "F3D configuration variant `{name}` parameter overrides must be JSON scalars: {entry_name}"
            )));
        }
        if let Some(suppressed) = definition.get("suppressed") {
            let valid = suppressed
                .as_array()
                .is_some_and(|values| values.iter().all(serde_json::Value::is_string));
            if !valid {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration variant `{name}` suppressed list must contain strings: {entry_name}"
                )));
            }
        }
        if definition
            .get("material")
            .is_some_and(|value| !value.is_string())
        {
            return Err(CodecError::malformed(format_args!(
                "F3D configuration variant `{name}` material must be a string: {entry_name}"
            )));
        }
    }
    Ok(())
}

/// Validate that a native table's retained member order names every variant
/// exactly once. A missing order remains valid for a legacy native record only
/// when the table has at most one variant, whose position is unambiguous.
pub(crate) fn validate_configuration_variant_order(
    configuration: &DesignConfiguration,
) -> Result<(), CodecError> {
    if configuration.kind == DesignConfigurationKind::Rule {
        return configuration
            .variant_order
            .is_empty()
            .then_some(())
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D configuration rule carries a variant order: {}",
                    configuration.entry_name
                ))
            });
    }
    let variants = configuration
        .payload
        .get("configurations")
        .and_then(serde_json::Value::as_object);
    let count = variants.map_or(0, serde_json::Map::len);
    if configuration.variant_order.is_empty() && count <= 1 {
        return Ok(());
    }
    let mut unique = HashSet::with_capacity(configuration.variant_order.len());
    let valid = variants.is_some_and(|variants| {
        configuration.variant_order.len() == variants.len()
            && configuration
                .variant_order
                .iter()
                .all(|name| unique.insert(name) && variants.contains_key(name))
    });
    valid.then_some(()).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3D configuration variant order does not match its table: {}",
            configuration.entry_name
        ))
    })
}

fn ordered_configuration_variants(
    configuration: &DesignConfiguration,
) -> Vec<(&String, &serde_json::Value)> {
    let Some(variants) = configuration
        .payload
        .get("configurations")
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };
    if configuration.variant_order.is_empty() {
        return variants.iter().collect();
    }
    configuration
        .variant_order
        .iter()
        .map(|name| {
            (
                name,
                variants
                    .get(name)
                    .expect("validated configuration order names a table member"),
            )
        })
        .collect()
}

struct OrderedConfigurationVariants<'a> {
    configuration: &'a DesignConfiguration,
    variants: &'a serde_json::Map<String, serde_json::Value>,
}

impl Serialize for OrderedConfigurationVariants<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.variants.len()))?;
        if self.configuration.variant_order.is_empty() {
            for (name, value) in self.variants {
                map.serialize_entry(name, value)?;
            }
        } else {
            for name in &self.configuration.variant_order {
                map.serialize_entry(name, &self.variants[name])?;
            }
        }
        map.end()
    }
}

struct OrderedConfigurationPayload<'a>(&'a DesignConfiguration);

impl Serialize for OrderedConfigurationPayload<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let object = self
            .0
            .payload
            .as_object()
            .expect("validated configuration payload is an object");
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for (name, value) in object {
            if name == "configurations" && self.0.kind == DesignConfigurationKind::Table {
                let variants = value
                    .as_object()
                    .expect("validated configuration variants are an object");
                map.serialize_entry(
                    name,
                    &OrderedConfigurationVariants {
                        configuration: self.0,
                        variants,
                    },
                )?;
            } else {
                map.serialize_entry(name, value)?;
            }
        }
        map.end()
    }
}

/// Encode a configuration document while retaining authored variant order.
pub(crate) fn encode_configuration_payload(
    configuration: &DesignConfiguration,
) -> Result<Vec<u8>, CodecError> {
    validate_configuration_payload(
        &configuration.entry_name,
        configuration.kind,
        &configuration.payload,
    )?;
    validate_configuration_variant_order(configuration)?;
    serde_json::to_vec(&OrderedConfigurationPayload(configuration)).map_err(|error| {
        CodecError::malformed(format_args!(
            "cannot encode F3D configuration JSON {}: {error}",
            configuration.entry_name
        ))
    })
}

/// Project named variants from configuration-table JSON into the neutral
/// configuration arena. Rule documents remain in the native arena because a
/// rule is a selector, not a model variant.
pub fn project_configurations(
    native: &[DesignConfiguration],
) -> Result<Vec<cadmpeg_ir::features::DesignConfiguration>, CodecError> {
    use cadmpeg_ir::features::DesignConfiguration as NeutralConfiguration;
    use std::collections::BTreeMap;

    for configuration in native {
        validate_configuration_payload(
            &configuration.entry_name,
            configuration.kind,
            &configuration.payload,
        )?;
        validate_configuration_variant_order(configuration)?;
    }
    if native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Table)
        .filter(|configuration| {
            configuration
                .payload
                .get("configurations")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|variants| !variants.is_empty())
        })
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(
            "independent nonempty F3D configuration tables have no shared authored order".into(),
        ));
    }

    let mut projected = Vec::new();
    for table in native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Table)
    {
        let active = table
            .payload
            .get("active")
            .and_then(serde_json::Value::as_str);
        for (name, definition) in ordered_configuration_variants(table) {
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
            let ordinal = u32::try_from(projected.len()).map_err(|_| {
                CodecError::Malformed("F3D configuration ordinal exceeds u32".into())
            })?;
            projected.push(NeutralConfiguration {
                id: neutral_configuration_id(&table.entry_name, name),
                ordinal,
                active: active == Some(name.as_str()),
                source_index: None,
                name: name.clone().into(),
                material,
                properties,
                parameter_overrides: BTreeMap::new(),
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
    projected.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(projected)
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
            configuration.feature_states.insert(
                feature.id.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState {
                    suppressed: true,
                    dependencies: feature.dependencies.clone(),
                    outputs: Vec::new(),
                    definition: feature.definition.clone(),
                },
            );
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
        encode_configuration_payload, parse_configuration_variant_order, project_configurations,
        unresolved_configuration_member_count, unresolved_configuration_parameter_override_count,
        unresolved_configuration_rule_count, unresolved_configuration_suppressed_feature_count,
        validate_configuration_payload, validate_configuration_variant_order,
    };
    use crate::records::{DesignConfiguration, DesignConfigurationKind};
    use cadmpeg_ir::features::{
        DesignParameter as NeutralParameter, Feature, FeatureDefinition, FeatureId, ParameterId,
    };
    use std::collections::BTreeMap;

    #[test]
    fn configuration_variants_follow_serialized_member_order() {
        let bytes = br#"{"configurations":{"Small":{},"Medium":{},"Large":{}},"active":"Medium"}"#;
        let payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let variant_order = parse_configuration_variant_order("table.dsgcfg", bytes).unwrap();
        assert_eq!(variant_order, ["Small", "Medium", "Large"]);

        let table = DesignConfiguration {
            id: "f3d:configuration:entry#table.dsgcfg".into(),
            entry_name: "table.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            variant_order,
            payload,
        };
        let projected = project_configurations(std::slice::from_ref(&table)).unwrap();
        let mut authored = projected
            .iter()
            .filter_map(|configuration| {
                Some((configuration.name.resolved()?, configuration.ordinal))
            })
            .collect::<Vec<_>>();
        authored.sort_by_key(|(_, ordinal)| *ordinal);
        assert_eq!(authored, [("Small", 0), ("Medium", 1), ("Large", 2)]);
        let encoded = encode_configuration_payload(&table).unwrap();
        assert_eq!(
            parse_configuration_variant_order("table.dsgcfg", &encoded).unwrap(),
            ["Small", "Medium", "Large"]
        );

        let mut incomplete = table;
        incomplete.variant_order.pop();
        assert!(validate_configuration_variant_order(&incomplete).is_err());
        assert!(parse_configuration_variant_order(
            "table.dsgcfg",
            br#"{"configurations":{"Small":{},"Small":{}}}"#,
        )
        .is_err());
    }

    #[test]
    fn configuration_identity_is_stable_across_table_order_and_delimiter_names() {
        let first = crate::ids::neutral_configuration_id("asset/a#b.dsgcfg", "c");
        let second = crate::ids::neutral_configuration_id("asset/a.dsgcfg", "b#c");
        assert_ne!(first, second);
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
                variant_order: vec!["variant".into()],
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
                variant_order: Vec::new(),
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
            variant_order: Vec::new(),
            payload: serde_json::json!({"when": "width > 20 mm", "vendorExtension": 7}),
        }];
        assert!(validate_configuration_payload(
            "partial.dsgcfgrule",
            DesignConfigurationKind::Rule,
            &native[0].payload,
        )
        .is_ok());
        let projected = project_configurations(&native).expect("empty rule projection");
        assert!(projected.is_empty());
        assert_eq!(unresolved_configuration_rule_count(&native, &projected), 1);
    }

    #[test]
    fn configuration_rules_bind_only_one_named_variant() {
        let table = |entry_name: &str, variant_name: &str| DesignConfiguration {
            id: format!("f3d:configuration:entry#{entry_name}"),
            entry_name: entry_name.into(),
            kind: DesignConfigurationKind::Table,
            variant_order: vec![variant_name.into()],
            payload: serde_json::json!({"configurations": {variant_name: {}}}),
        };
        let rule = DesignConfiguration {
            id: "f3d:configuration:entry#rule.dsgcfgrule".into(),
            entry_name: "rule.dsgcfgrule".into(),
            kind: DesignConfigurationKind::Rule,
            variant_order: Vec::new(),
            payload: serde_json::json!({"when": "width > 20 mm", "activate": "wide"}),
        };
        let native = [table("table.dsgcfg", "wide"), rule.clone()];
        let projected = project_configurations(&native).expect("ordered configuration table");
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
        let error = project_configurations(&ambiguous)
            .expect_err("independent nonempty tables have no shared order");
        assert!(error
            .to_string()
            .contains("configuration tables have no shared authored order"));
    }

    #[test]
    fn configuration_parameter_overrides_bind_only_unique_parameter_names() {
        let table = DesignConfiguration {
            id: "f3d:configuration:entry#table.dsgcfg".into(),
            entry_name: "table.dsgcfg".into(),
            kind: DesignConfigurationKind::Table,
            variant_order: vec!["wide".into()],
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
        let mut projected = project_configurations(&[table]).expect("ordered configuration table");
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
            variant_order: vec!["wide".into()],
            payload: serde_json::json!({
                "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
            }),
        }])
        .expect("ordered configuration table");
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
            variant_order: vec!["alternate".into()],
            payload: serde_json::json!({
                "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
            }),
        };
        let feature = Feature {
            id: FeatureId("f3d:model:feature#fillet-1".into()),
            ordinal: 0,
            name: Some("Fillet 1".into()),
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "Fillet".into(),
                parameters: BTreeMap::new(),
            },
            native_ref: None,
        };
        let mut projected = project_configurations(&[table]).expect("ordered configuration table");
        bind_configuration_suppressed_features(&mut projected, std::slice::from_ref(&feature));
        assert_eq!(
            projected[0].suppressed_features().collect::<Vec<_>>(),
            [&feature.id]
        );
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
            variant_order: vec!["alternate".into()],
            payload: serde_json::json!({
                "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
            }),
        }])
        .expect("ordered configuration table");
        bind_configuration_suppressed_features(&mut ambiguous, &[feature, duplicate]);
        assert!(ambiguous[0].suppressed_features().next().is_none());
        assert_eq!(
            unresolved_configuration_suppressed_feature_count(&ambiguous),
            1
        );
    }
}
