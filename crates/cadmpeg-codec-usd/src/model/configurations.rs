// SPDX-License-Identifier: Apache-2.0
//! Design configurations from `CadConfiguration` prims (spec §18.8).

use std::collections::BTreeSet;

use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};

use super::features::strings;
use crate::schema::{boolean, integer, token, tokens};
use crate::stage::Prim;

/// The type of a `CadConfiguration` prim.
pub(super) const CONFIGURATION_TYPE: &str = "CadConfiguration";

impl super::Builder<'_, '_, '_, '_> {
    /// Adds the configuration of each `CadConfiguration` prim, in stage
    /// order, after the features and parameters.
    pub(super) fn configurations(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let stage = self.stage;
        for prim in stage
            .prims
            .iter()
            .filter(|prim| prim.type_name() == Some(CONFIGURATION_TYPE))
        {
            match self.configuration(prim) {
                Ok(configuration) => {
                    self.ir.model.configurations.push(configuration);
                    self.admit("usd_configuration")?;
                }
                Err(reason) => self
                    .tallies
                    .rejected_features
                    .push(format!("{}: {reason}", prim.path)),
            }
        }
        Ok(())
    }

    fn configuration(&mut self, prim: &Prim<'_>) -> Result<DesignConfiguration, String> {
        let id = self
            .cad_id(&prim.path, |text| ConfigurationId::mint(text).ok())
            .unwrap_or_else(|| crate::ids::configuration(&prim.path));
        let mut wire = match token(prim, "cad:configuration:payload", &mut self.issues) {
            Some(payload) => match serde_json::from_str(&payload)
                .map_err(|error| format!("cad:configuration:payload: {error}"))?
            {
                serde_json::Value::Object(wire) => wire,
                _ => return Err("cad:configuration:payload is not a JSON object".to_owned()),
            },
            None => serde_json::Map::new(),
        };
        // Every feature and parameter identity of the stage that the payload
        // names is one the model holds.
        let model = &self.ir.model;
        let held: BTreeSet<String> = model
            .features
            .iter()
            .map(|feature| feature.id.to_string())
            .chain(
                model
                    .parameters
                    .iter()
                    .map(|parameter| parameter.id.to_string()),
            )
            .chain(model.bodies.iter().map(|body| body.id.to_string()))
            .collect();
        let named: BTreeSet<String> = self
            .feature_ids
            .values()
            .map(ToString::to_string)
            .chain(self.parameter_ids.values().map(ToString::to_string))
            .collect();
        let mut leaves = Vec::new();
        for value in wire.values() {
            strings(value, &mut leaves);
        }
        if let Some(missing) = leaves
            .iter()
            .find(|text| named.contains(**text) && !held.contains(**text))
        {
            return Err(format!("`{missing}` is not transferred"));
        }
        if let Some(serde_json::Value::Array(bodies)) = wire.get_mut("bodies") {
            let listed = bodies.len();
            bodies.retain(|body| body.as_str().is_some_and(|body| held.contains(body)));
            if bodies.len() < listed {
                self.tallies.feature_properties.push(format!(
                    "{}: {} bodies",
                    prim.path,
                    listed - bodies.len()
                ));
            }
        }
        let names =
            tokens(prim, "cad:configuration:propertyNames", &mut self.issues).unwrap_or_default();
        let values =
            tokens(prim, "cad:configuration:propertyValues", &mut self.issues).unwrap_or_default();
        if names.len() != values.len() {
            return Err("the property names and values differ in length".to_owned());
        }
        let mut insert = |key: &str, value: serde_json::Value| {
            wire.insert(key.to_owned(), value);
        };
        insert("id", id.to_string().into());
        insert(
            "ordinal",
            integer(prim, "cad:configuration:ordinal", &mut self.issues)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0)
                .into(),
        );
        insert(
            "active",
            boolean(prim, "cad:configuration:active", &mut self.issues)
                .unwrap_or(false)
                .into(),
        );
        if let Some(index) = integer(prim, "cad:configuration:sourceIndex", &mut self.issues)
            .and_then(|value| u32::try_from(value).ok())
        {
            insert("source_index", index.into());
        }
        for (key, attribute) in [
            ("name", "cad:configuration:name"),
            ("material", "cad:configuration:material"),
        ] {
            if let Some(text) = token(prim, attribute, &mut self.issues) {
                insert(key, text.into());
            }
        }
        if !names.is_empty() {
            insert(
                "properties",
                names
                    .into_iter()
                    .zip(values)
                    .map(|(name, value)| (name, serde_json::Value::from(value)))
                    .collect::<serde_json::Map<_, _>>()
                    .into(),
            );
        }
        serde_json::from_value(serde_json::Value::Object(wire))
            .map_err(|error| format!("the configuration does not deserialize: {error}"))
    }
}
