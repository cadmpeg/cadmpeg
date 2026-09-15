// SPDX-License-Identifier: Apache-2.0
//! Admitted configuration documents and their authored variant order.

use cadmpeg_core::CodecError;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// JSON configuration payload stored in a Fusion design-configuration entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignConfigurationWire", into = "DesignConfigurationWire")]
pub(crate) struct DesignConfiguration {
    entry_name: String,
    identity_scope: cadmpeg_ir::ids::IdentityComponent,
    payload: ConfigurationPayload,
}

#[derive(Debug, Clone, PartialEq)]
enum ConfigurationPayload {
    Table {
        active: Option<String>,
        variants: Option<ConfigurationVariants>,
        extensions: Map<String, Value>,
    },
    Rule(Map<String, Value>),
}

/// A table's actual members, in authored order.
#[derive(Debug, Clone, PartialEq)]
struct ConfigurationVariants {
    entries: Vec<(String, ConfigurationVariant)>,
    /// Empty or singleton legacy records can omit their unambiguous order.
    explicit_order: bool,
}

/// Typed variant properties and retained extension members.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfigurationVariant {
    parameters: Option<BTreeMap<String, ConfigurationScalar>>,
    suppressed: Option<Vec<String>>,
    material: Option<String>,
    extensions: Map<String, Value>,
}

/// A parameter override contains one JSON scalar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConfigurationScalar {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
}

impl ConfigurationScalar {
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::String(value) => value.clone(),
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(*value),
            Self::Number(value) => Value::Number(value.clone()),
            Self::String(value) => Value::String(value.clone()),
        }
    }
}

impl ConfigurationVariant {
    fn admit(entry_name: &str, name: &str, value: Value) -> Result<Self, CodecError> {
        let Value::Object(mut fields) = value else {
            return Err(CodecError::malformed(format_args!(
                "F3D configuration variant `{name}` must be an object: {entry_name}"
            )));
        };
        let parameters = match fields.remove("parameters") {
            Some(Value::Object(parameters)) => {
                let mut admitted = BTreeMap::new();
                for (key, value) in parameters {
                    let value = match value {
                        Value::Null => ConfigurationScalar::Null,
                        Value::Bool(value) => ConfigurationScalar::Bool(value),
                        Value::Number(value) => ConfigurationScalar::Number(value),
                        Value::String(value) => ConfigurationScalar::String(value),
                        Value::Array(_) | Value::Object(_) => {
                            return Err(CodecError::malformed(format_args!(
                                "F3D configuration variant `{name}` parameter overrides must be JSON scalars: {entry_name}"
                            )));
                        }
                    };
                    admitted.insert(key, value);
                }
                Some(admitted)
            }
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration variant `{name}` parameters must be an object: {entry_name}"
                )))
            }
            None => None,
        };
        let suppressed_error = || {
            CodecError::malformed(format_args!(
            "F3D configuration variant `{name}` suppressed list must contain strings: {entry_name}"
        ))
        };
        let suppressed = match fields.remove("suppressed") {
            Some(Value::Array(values)) => Some(
                values
                    .into_iter()
                    .map(|value| match value {
                        Value::String(value) => Ok(value),
                        _ => Err(suppressed_error()),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Some(_) => return Err(suppressed_error()),
            None => None,
        };
        let material = match fields.remove("material") {
            Some(Value::String(value)) => Some(value),
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration variant `{name}` material must be a string: {entry_name}"
                )))
            }
            None => None,
        };
        Ok(Self {
            parameters,
            suppressed,
            material,
            extensions: fields,
        })
    }

    pub(crate) fn parameters(&self) -> impl Iterator<Item = (&String, &ConfigurationScalar)> {
        self.parameters
            .iter()
            .flat_map(|parameters| parameters.iter())
    }

    pub(crate) fn suppressed(&self) -> impl Iterator<Item = &String> {
        self.suppressed.iter().flatten()
    }

    pub(crate) fn material(&self) -> Option<&str> {
        self.material.as_deref()
    }

    fn payload(&self) -> Map<String, Value> {
        let mut payload = self.extensions.clone();
        if let Some(parameters) = &self.parameters {
            payload.insert(
                "parameters".into(),
                Value::Object(
                    parameters
                        .iter()
                        .map(|(name, value)| (name.clone(), value.value()))
                        .collect(),
                ),
            );
        }
        if let Some(suppressed) = &self.suppressed {
            payload.insert(
                "suppressed".into(),
                Value::Array(suppressed.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(material) = &self.material {
            payload.insert("material".into(), Value::String(material.clone()));
        }
        payload
    }
}

/// Serialized configuration identity and payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignConfigurationWire {
    pub id: String,
    pub entry_name: String,
    pub kind: DesignConfigurationKind,
    #[serde(default)]
    pub variant_order: Vec<String>,
    pub payload: Value,
}

/// Native Fusion design-configuration entry family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignConfigurationKind {
    Table,
    Rule,
}

impl DesignConfiguration {
    /// Admit the entry identity, object payload, and authored variant order.
    pub(crate) fn try_new(
        entry_name: String,
        kind: DesignConfigurationKind,
        variant_order: Vec<String>,
        mut payload: Map<String, Value>,
    ) -> Result<Self, CodecError> {
        let extension = match kind {
            DesignConfigurationKind::Table => ".dsgcfg",
            DesignConfigurationKind::Rule => ".dsgcfgrule",
        };
        if !entry_name.ends_with(extension) {
            return Err(CodecError::malformed(format_args!(
                "configuration.entry_name must end with {extension} for {kind:?}"
            )));
        }
        if kind == DesignConfigurationKind::Rule {
            if !variant_order.is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration rule carries a variant order: {entry_name}"
                )));
            }
            return Ok(Self {
                entry_name,
                identity_scope: cadmpeg_ir::identity_component!("configuration"),
                payload: ConfigurationPayload::Rule(payload),
            });
        }
        let variants = match payload.remove("configurations") {
            Some(Value::Object(variants)) => Some(variants),
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration table `configurations` must be an object: {entry_name}"
                )))
            }
            None => None,
        };
        let active = match payload.remove("active") {
            Some(Value::String(active)) => {
                if !variants
                    .as_ref()
                    .is_some_and(|variants| variants.contains_key(&active))
                {
                    return Err(CodecError::malformed(format_args!(
                        "F3D active configuration `{active}` is not a named variant: {entry_name}"
                    )));
                }
                Some(active)
            }
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration table `active` must be a string: {entry_name}"
                )))
            }
            None => None,
        };
        let invalid_order = || {
            CodecError::malformed(format_args!(
                "F3D configuration variant order does not match its table: {entry_name}"
            ))
        };
        let variants = match variants {
            Some(variants) => {
                let mut variants = variants
                    .into_iter()
                    .map(|(name, value)| {
                        ConfigurationVariant::admit(&entry_name, &name, value)
                            .map(|value| (name, value))
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                let explicit_order = !variant_order.is_empty();
                let entries = if !explicit_order && variants.len() <= 1 {
                    variants.into_iter().collect()
                } else {
                    let entries = variant_order
                        .into_iter()
                        .map(|name| variants.remove_entry(&name).ok_or_else(&invalid_order))
                        .collect::<Result<_, _>>()?;
                    if !variants.is_empty() {
                        return Err(invalid_order());
                    }
                    entries
                };
                Some(ConfigurationVariants {
                    entries,
                    explicit_order,
                })
            }
            None if variant_order.is_empty() => None,
            None => return Err(invalid_order()),
        };
        Ok(Self {
            entry_name,
            identity_scope: cadmpeg_ir::identity_component!("configuration"),
            payload: ConfigurationPayload::Table {
                active,
                variants,
                extensions: payload,
            },
        })
    }

    pub(crate) fn id(&self) -> String {
        crate::ids::configuration_entry_id(&self.entry_name, &self.identity_scope)
    }

    pub(crate) fn entry_name(&self) -> &String {
        &self.entry_name
    }

    pub(crate) fn kind(&self) -> DesignConfigurationKind {
        match self.payload {
            ConfigurationPayload::Table { .. } => DesignConfigurationKind::Table,
            ConfigurationPayload::Rule(_) => DesignConfigurationKind::Rule,
        }
    }

    /// Actual named definitions in authored order; rules have no variants.
    pub(crate) fn variants(&self) -> &[(String, ConfigurationVariant)] {
        match &self.payload {
            ConfigurationPayload::Table {
                variants: Some(variants),
                ..
            } => &variants.entries,
            _ => &[],
        }
    }

    pub(crate) fn active(&self) -> Option<&str> {
        match &self.payload {
            ConfigurationPayload::Table { active, .. } => active.as_deref(),
            ConfigurationPayload::Rule(_) => None,
        }
    }

    pub(crate) fn rule(&self) -> Option<&Map<String, Value>> {
        match &self.payload {
            ConfigurationPayload::Rule(payload) => Some(payload),
            ConfigurationPayload::Table { .. } => None,
        }
    }

    /// The native wire keeps a missing legacy order as an empty list.
    pub(crate) fn variant_order(&self) -> Vec<String> {
        match &self.payload {
            ConfigurationPayload::Table {
                variants: Some(variants),
                ..
            } if variants.explicit_order => variants
                .entries
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Rebuild the complete object payload, including extension members.
    pub(crate) fn payload(&self) -> Map<String, Value> {
        match &self.payload {
            ConfigurationPayload::Rule(payload) => payload.clone(),
            ConfigurationPayload::Table {
                active,
                variants,
                extensions,
            } => {
                let mut payload = extensions.clone();
                if let Some(active) = active {
                    payload.insert("active".into(), Value::String(active.clone()));
                }
                if let Some(variants) = variants {
                    payload.insert(
                        "configurations".into(),
                        Value::Object(
                            variants
                                .entries
                                .iter()
                                .map(|(name, value)| (name.clone(), Value::Object(value.payload())))
                                .collect(),
                        ),
                    );
                }
                payload
            }
        }
    }

    pub(crate) fn unknown_member_count(&self) -> usize {
        match &self.payload {
            ConfigurationPayload::Rule(payload) => payload
                .keys()
                .filter(|key| !matches!(key.as_str(), "when" | "activate"))
                .count(),
            ConfigurationPayload::Table { extensions, .. } => {
                extensions.len()
                    + self
                        .variants()
                        .iter()
                        .map(|(_, variant)| variant.extensions.len())
                        .sum::<usize>()
            }
        }
    }
}

impl TryFrom<DesignConfigurationWire> for DesignConfiguration {
    type Error = String;
    fn try_from(wire: DesignConfigurationWire) -> Result<Self, String> {
        let suffix = format!(
            ":entry#{}",
            crate::ids::identity_key_component(&wire.entry_name)
        );
        let scope = wire
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.strip_suffix(&suffix))
            .ok_or_else(|| format!("configuration.id {} must identify entry_name", wire.id))?;
        let scope = cadmpeg_ir::ids::IdentityComponent::try_new(scope)
            .map_err(|error| format!("configuration.id {}: {error}", wire.id))?;
        let Value::Object(payload) = wire.payload else {
            return Err("payload must be an object".into());
        };
        let mut record = Self::try_new(wire.entry_name, wire.kind, wire.variant_order, payload)
            .map_err(|error| error.to_string())?;
        record.identity_scope = scope;
        Ok(record)
    }
}

impl From<DesignConfiguration> for DesignConfigurationWire {
    fn from(value: DesignConfiguration) -> Self {
        Self {
            id: value.id(),
            kind: value.kind(),
            variant_order: value.variant_order(),
            payload: Value::Object(value.payload()),
            entry_name: value.entry_name,
        }
    }
}

struct OrderedConfigurationVariants<'a>(&'a [(String, ConfigurationVariant)]);

impl Serialize for OrderedConfigurationVariants<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0 {
            map.serialize_entry(name, &value.payload())?;
        }
        map.end()
    }
}

struct OrderedConfigurationPayload<'a>(&'a DesignConfiguration);

impl Serialize for OrderedConfigurationPayload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let object = self.0.payload();
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for (name, value) in object {
            if name == "configurations" && self.0.kind() == DesignConfigurationKind::Table {
                map.serialize_entry(&name, &OrderedConfigurationVariants(self.0.variants()))?;
            } else {
                map.serialize_entry(&name, &value)?;
            }
        }
        map.end()
    }
}

/// Encode the native source payload with its authored variant order.
pub(crate) fn encode_configuration_payload(
    configuration: &DesignConfiguration,
) -> Result<Vec<u8>, CodecError> {
    serde_json::to_vec(&OrderedConfigurationPayload(configuration)).map_err(|error| {
        CodecError::malformed(format_args!(
            "cannot encode F3D configuration JSON {}: {error}",
            configuration.entry_name()
        ))
    })
}

#[cfg(test)]
mod tests;
