// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::cloned_ref_to_slice_refs))]
//! Decode and project Design configuration records.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::ids::neutral_configuration_id;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use cadmpeg_core::CodecError;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
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
pub(crate) fn decode_configurations(
    scan: &ContainerScan,
) -> Result<Vec<DesignConfiguration>, CodecError> {
    let configurations = scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::DesignConfig))
        .map(|entry| {
            let bytes = scan.entry_bytes(&entry.name)?;
            let payload: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
                CodecError::malformed(format_args!(
                    "invalid F3D configuration JSON {}: {error}",
                    entry.name
                ))
            })?;
            let serde_json::Value::Object(payload) = payload else {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration JSON must be an object: {}",
                    entry.name
                )));
            };
            let kind = if entry.name.ends_with(".dsgcfgrule") {
                DesignConfigurationKind::Rule
            } else {
                DesignConfigurationKind::Table
            };
            let variant_order = if kind == DesignConfigurationKind::Table {
                parse_configuration_variant_order(&entry.name, bytes)?
            } else {
                Vec::new()
            };
            DesignConfiguration::try_new(entry.name.clone(), kind, variant_order, payload)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut names = HashSet::new();
    for configuration in &configurations {
        if !names.insert(configuration.entry_name().as_str()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D configuration identity: {}",
                configuration.entry_name()
            )));
        }
    }
    Ok(configurations)
}

/// Project named variants from configuration-table JSON into the neutral
/// configuration arena. Rule documents remain in the native arena because a
/// rule is a selector, not a model variant.
pub(crate) fn project_configurations(
    native: &[DesignConfiguration],
) -> Result<Vec<cadmpeg_ir::features::DesignConfiguration>, CodecError> {
    use cadmpeg_ir::features::DesignConfiguration as NeutralConfiguration;
    use std::collections::BTreeMap;

    if native
        .iter()
        .filter(|configuration| !configuration.variants().is_empty())
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(
            "independent nonempty F3D configuration tables have no shared authored order".into(),
        ));
    }

    let mut projected = Vec::new();
    for table in native {
        let active = table.active();
        for (name, definition) in table.variants() {
            let mut properties = BTreeMap::new();
            for (parameter, value) in definition.parameters() {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("parameter:{}", parameter),
                    value.text(),
                );
            }
            for feature in definition.suppressed() {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("suppressed:{}", feature),
                    "true".into(),
                );
            }
            let material = definition.material().map(str::to_owned);
            let ordinal = u32::try_from(projected.len()).map_err(|_| {
                CodecError::Malformed("F3D configuration ordinal exceeds u32".into())
            })?;
            projected.push(NeutralConfiguration {
                id: neutral_configuration_id(table.entry_name(), name),
                ordinal,
                active: active == Some(name.as_str()),
                source_index: None,
                name: name.clone().into(),
                material,
                properties,
                parameter_overrides: BTreeMap::new(),
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                bodies: None,
                native_ref: Some(table.id()),
            });
        }
    }
    for (rule, payload) in native.iter().filter_map(|rule| Some((rule, rule.rule()?))) {
        let Some(condition) = payload.get("when").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = payload.get("activate").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let mut matches = projected
            .iter_mut()
            .filter(|configuration| configuration.name.as_deref() == Some(target));
        let Some(configuration) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        configuration.properties.insert(
            cadmpeg_core::nonblank_literal!("activation_rule:{}", rule.entry_name()),
            condition.to_owned(),
        );
    }
    projected.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(projected)
}

/// Replace name-keyed configuration properties with stable parameter references
/// when exactly one neutral parameter has the named source identity.
pub(crate) fn bind_configuration_parameter_overrides(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) {
    for configuration in configurations {
        configuration.properties.retain(|key, expression| {
            let Some(name) = key.as_str().strip_prefix("parameter:") else {
                return true;
            };
            let mut matches = parameters.iter().filter(|parameter| parameter.name == name);
            let Some(parameter) = matches.next() else {
                return true;
            };
            if matches.next().is_some() {
                return true;
            }
            configuration
                .parameter_overrides
                .insert(parameter.id.clone(), std::mem::take(expression));
            false
        });
    }
}

/// Replace name-keyed suppression properties with stable feature references
/// when exactly one neutral feature has the named source identity.
pub(crate) fn bind_configuration_suppressed_features(
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    features: &[cadmpeg_ir::features::Feature],
) {
    for configuration in configurations {
        configuration.properties.retain(|key, _| {
            let Some(name) = key.as_str().strip_prefix("suppressed:") else {
                return true;
            };
            let mut matches = features
                .iter()
                .filter(|feature| feature.name.as_deref() == Some(name));
            let Some(feature) = matches.next() else {
                return true;
            };
            if matches.next().is_some() {
                return true;
            }
            configuration.feature_states.insert(
                feature.id.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Suppressed {},
                    dependencies: feature.dependencies.clone(),
                    definition: feature.evaluation.definition().clone(),
                },
            );
            false
        });
    }
}

pub(crate) fn unresolved_configuration_parameter_override_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.as_str().starts_with("parameter:"))
        .count()
}

pub(crate) fn unresolved_configuration_suppressed_feature_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.as_str().starts_with("suppressed:"))
        .count()
}

pub(crate) fn unresolved_configuration_rule_count(
    native: &[DesignConfiguration],
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    native
        .iter()
        .filter(|rule| rule.rule().is_some_and(|payload| !payload.is_empty()))
        .filter(|rule| {
            !projected.iter().any(|configuration| {
                configuration
                    .properties
                    .contains_key(format!("activation_rule:{}", rule.entry_name()).as_str())
            })
        })
        .count()
}

pub(crate) fn unresolved_configuration_member_count(native: &[DesignConfiguration]) -> usize {
    native
        .iter()
        .map(DesignConfiguration::unknown_member_count)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{
        bind_configuration_parameter_overrides, bind_configuration_suppressed_features,
        parse_configuration_variant_order, project_configurations,
        unresolved_configuration_member_count, unresolved_configuration_parameter_override_count,
        unresolved_configuration_rule_count, unresolved_configuration_suppressed_feature_count,
    };
    use crate::records::configuration::{
        encode_configuration_payload, DesignConfiguration, DesignConfigurationKind,
    };
    use cadmpeg_ir::features::{
        DesignParameter as NeutralParameter, Feature, FeatureDefinition, FeatureId,
        FeatureOperation, ParameterId,
    };
    use std::collections::BTreeMap;

    #[test]
    fn configuration_variants_follow_serialized_member_order() {
        let bytes = br#"{"configurations":{"Small":{},"Medium":{},"Large":{}},"active":"Medium"}"#;
        let payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let variant_order = parse_configuration_variant_order("table.dsgcfg", bytes).unwrap();
        assert_eq!(variant_order, ["Small", "Medium", "Large"]);

        let table = DesignConfiguration::try_new(
            "table.dsgcfg".into(),
            DesignConfigurationKind::Table,
            variant_order,
            (payload).as_object().unwrap().clone(),
        )
        .unwrap();
        let projected = project_configurations(std::slice::from_ref(&table)).unwrap();
        let mut authored = projected
            .iter()
            .filter_map(|configuration| {
                Some((configuration.name.as_deref()?, configuration.ordinal))
            })
            .collect::<Vec<_>>();
        authored.sort_by_key(|(_, ordinal)| *ordinal);
        assert_eq!(authored, [("Small", 0), ("Medium", 1), ("Large", 2)]);
        let encoded = encode_configuration_payload(&table).unwrap();
        assert_eq!(
            parse_configuration_variant_order("table.dsgcfg", &encoded).unwrap(),
            ["Small", "Medium", "Large"]
        );

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
    fn configuration_unknown_members_are_counted_at_each_semantic_level() {
        let native = [
            DesignConfiguration::try_new(
                "table.dsgcfg".into(),
                DesignConfigurationKind::Table,
                vec!["variant".into()],
                (serde_json::json!({
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
                }))
                .as_object()
                .unwrap()
                .clone(),
            )
            .unwrap(),
            DesignConfiguration::try_new(
                "rule.dsgcfgrule".into(),
                DesignConfigurationKind::Rule,
                Vec::new(),
                (serde_json::json!({
                    "when": "width > 20 mm",
                    "activate": "variant",
                    "rule_unknown": null
                }))
                .as_object()
                .unwrap()
                .clone(),
            )
            .unwrap(),
        ];
        assert_eq!(unresolved_configuration_member_count(&native), 3);
    }

    #[test]
    fn configuration_rule_without_the_typed_pair_is_retained_not_rejected() {
        let native = [DesignConfiguration::try_new(
            "partial.dsgcfgrule".into(),
            DesignConfigurationKind::Rule,
            Vec::new(),
            (serde_json::json!({"when": "width > 20 mm", "vendorExtension": 7}))
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap()];
        let projected = project_configurations(&native).expect("empty rule projection");
        assert!(projected.is_empty());
        assert_eq!(unresolved_configuration_rule_count(&native, &projected), 1);
    }

    #[test]
    fn configuration_rules_bind_only_one_named_variant() {
        let table = |entry_name: &str, variant_name: &str| {
            DesignConfiguration::try_new(
                entry_name.into(),
                DesignConfigurationKind::Table,
                vec![variant_name.into()],
                (serde_json::json!({"configurations": {variant_name: {}}}))
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .unwrap()
        };
        let rule = DesignConfiguration::try_new(
            "rule.dsgcfgrule".into(),
            DesignConfigurationKind::Rule,
            Vec::new(),
            (serde_json::json!({"when": "width > 20 mm", "activate": "wide"}))
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap();
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
        let table = DesignConfiguration::try_new(
            "table.dsgcfg".into(),
            DesignConfigurationKind::Table,
            vec!["wide".into()],
            (serde_json::json!({
                "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
            }))
            .as_object()
            .unwrap()
            .clone(),
        )
        .unwrap();
        let parameter = NeutralParameter {
            id: ParameterId::mint("f3d:model:parameter#width").expect("identity grammar"),
            owner: None,
            ordinal: 0,
            name: "width".into(),
            expression: "10 mm".into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
            id: ParameterId::mint("f3d:model:parameter#other-width").expect("identity grammar"),
            ..parameter.clone()
        };
        let mut ambiguous = project_configurations(&[DesignConfiguration::try_new(
            "other.dsgcfg".into(),
            DesignConfigurationKind::Table,
            vec!["wide".into()],
            (serde_json::json!({
                "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
            }))
            .as_object()
            .unwrap()
            .clone(),
        )
        .unwrap()])
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
        let table = DesignConfiguration::try_new(
            "table.dsgcfg".into(),
            DesignConfigurationKind::Table,
            vec!["alternate".into()],
            (serde_json::json!({
                "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
            }))
            .as_object()
            .unwrap()
            .clone(),
        )
        .unwrap();
        let feature = Feature {
            id: FeatureId::mint("f3d:model:feature#fillet-1").expect("identity grammar"),
            ordinal: 0,
            name: Some("Fillet 1".into()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "Fillet".into(),
                    parameters: BTreeMap::new(),
                }),
            ),
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
            id: FeatureId::mint("f3d:model:feature#other-fillet-1").expect("identity grammar"),
            ..feature.clone()
        };
        let mut ambiguous = project_configurations(&[DesignConfiguration::try_new(
            "other.dsgcfg".into(),
            DesignConfigurationKind::Table,
            vec!["alternate".into()],
            (serde_json::json!({
                "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
            }))
            .as_object()
            .unwrap()
            .clone(),
        )
        .unwrap()])
        .expect("ordered configuration table");
        bind_configuration_suppressed_features(&mut ambiguous, &[feature, duplicate]);
        assert!(ambiguous[0].suppressed_features().next().is_none());
        assert_eq!(
            unresolved_configuration_suppressed_feature_count(&ambiguous),
            1
        );
    }
}
