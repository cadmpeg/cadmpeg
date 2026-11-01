// SPDX-License-Identifier: Apache-2.0
//! Construction features from `CadFeature` prims (spec §18.7).

use std::collections::BTreeSet;

use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureInputTopology, FeatureOperation,
    FeatureResultTopology,
};

use crate::schema::{boolean, integer, token, tokens};
use crate::stage::Prim;

/// The type of a `CadFeature` prim.
pub(super) const FEATURE_TYPE: &str = "CadFeature";

/// A feature read from its prim, with its topology records and the
/// features it names.
struct Read {
    feature: Feature,
    parent: Option<FeatureId>,
    inputs: Vec<FeatureInputTopology>,
    results: Vec<FeatureResultTopology>,
    references: BTreeSet<String>,
}

/// The string leaves and object keys of `value`.
pub(super) fn strings<'v>(value: &'v serde_json::Value, out: &mut Vec<&'v str>) {
    match value {
        serde_json::Value::String(text) => out.push(text),
        serde_json::Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                out.push(key);
                strings(item, out);
            }
        }
        _ => {}
    }
}

impl super::Builder<'_, '_, '_, '_> {
    /// Assigns the IR identity of each `CadFeature` prim of the stage.
    pub(super) fn feature_ids(&mut self) {
        let stage = self.stage;
        for prim in stage
            .prims
            .iter()
            .filter(|prim| prim.type_name() == Some(FEATURE_TYPE))
        {
            let id = self
                .cad_id(&prim.path, |text| FeatureId::mint(text).ok())
                .unwrap_or_else(|| crate::ids::feature(&prim.path));
            self.feature_ids.insert(prim.path.clone(), id);
        }
    }

    /// Adds the feature of each `CadFeature` prim, in stage order. A feature
    /// that names a feature the stage does not give is not added.
    pub(super) fn features(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let stage = self.stage;
        let mut reads = Vec::new();
        for prim in stage
            .prims
            .iter()
            .filter(|prim| prim.type_name() == Some(FEATURE_TYPE))
        {
            match self.feature(prim) {
                Ok(read) => reads.push((prim.path.clone(), read)),
                Err(reason) => self
                    .tallies
                    .rejected_features
                    .push(format!("{}: {reason}", prim.path)),
            }
        }
        loop {
            let given: BTreeSet<String> = reads
                .iter()
                .map(|(_, read)| read.feature.id.to_string())
                .collect();
            let before = reads.len();
            reads.retain(|(path, read)| {
                let missing = read
                    .references
                    .iter()
                    .find(|reference| !given.contains(*reference));
                if let Some(missing) = missing {
                    self.tallies
                        .rejected_features
                        .push(format!("{path}: feature `{missing}` is not transferred"));
                }
                missing.is_none()
            });
            if reads.len() == before {
                break;
            }
        }
        for (path, read) in reads {
            let id = read.feature.id.clone();
            self.ir.model.features.push(read.feature);
            if let Some(parent) = &read.parent {
                if let Err(error) = self.ir.model.set_feature_regeneration_parent(&id, parent) {
                    self.tallies
                        .rejected_features
                        .push(format!("{path}: {error}"));
                }
            }
            self.ir.model.feature_input_topologies.extend(read.inputs);
            self.ir.model.feature_result_topologies.extend(read.results);
            self.admit("usd_feature")?;
        }
        Ok(())
    }

    fn feature(&mut self, prim: &Prim<'_>) -> Result<Read, String> {
        let id = self
            .feature_ids
            .get(&prim.path)
            .cloned()
            .ok_or("the prim has no identity")?;
        let payload = token(prim, "cad:feature:payload", &mut self.issues)
            .ok_or("cad:feature:payload is not authored")?;
        let serde_json::Value::Object(mut wire) = serde_json::from_str(&payload)
            .map_err(|error| format!("cad:feature:payload: {error}"))?
        else {
            return Err("cad:feature:payload is not a JSON object".to_owned());
        };
        let feature_targets = |builder: &Self, name: &str| -> Result<Vec<FeatureId>, String> {
            prim.relationship_targets(name)
                .iter()
                .map(|target| {
                    builder
                        .feature_ids
                        .get(target)
                        .cloned()
                        .ok_or_else(|| format!("{name}: <{target}> is not a CadFeature prim"))
                })
                .collect()
        };
        let dependencies = feature_targets(self, "cad:feature:dependencies")?;
        let parent = match feature_targets(self, "cad:feature:regenerationParent")?.as_slice() {
            [] => None,
            [parent] => Some(parent.clone()),
            _ => return Err("cad:feature:regenerationParent has more than one target".to_owned()),
        };
        let mut outputs = tokens(prim, "cad:feature:outputs", &mut self.issues).unwrap_or_default();
        let bodies: BTreeSet<&str> = self
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.as_str())
            .collect();
        let listed = outputs.len();
        outputs.retain(|body| bodies.contains(body.as_str()));
        if outputs.len() < listed {
            self.tallies.feature_properties.push(format!(
                "{}: {} output bodies",
                prim.path,
                listed - outputs.len()
            ));
        }
        let known: BTreeSet<String> = self.feature_ids.values().map(ToString::to_string).collect();
        let mut leaves = Vec::new();
        for value in wire.values() {
            strings(value, &mut leaves);
        }
        let mut references: BTreeSet<String> = leaves
            .into_iter()
            .filter(|text| known.contains(*text))
            .map(str::to_owned)
            .collect();
        references.extend(dependencies.iter().map(ToString::to_string));
        references.extend(parent.iter().map(ToString::to_string));
        let names = tokens(prim, "cad:feature:propertyNames", &mut self.issues).unwrap_or_default();
        let values =
            tokens(prim, "cad:feature:propertyValues", &mut self.issues).unwrap_or_default();
        if names.len() != values.len() {
            return Err("the property names and values differ in length".to_owned());
        }
        let product_definition = self.definition_target(prim, "cad:feature:definition")?;
        let mut insert = |key: &str, value: serde_json::Value| {
            wire.insert(key.to_owned(), value);
        };
        insert("id", id.to_string().into());
        insert(
            "ordinal",
            integer(prim, "cad:feature:ordinal", &mut self.issues)
                .and_then(|value| u64::try_from(value).ok())
                .unwrap_or(0)
                .into(),
        );
        if let Some(name) = token(prim, "cad:feature:name", &mut self.issues) {
            insert("name", name.into());
        }
        insert(
            "suppressed",
            boolean(prim, "cad:feature:suppressed", &mut self.issues).into(),
        );
        if !dependencies.is_empty() {
            insert(
                "dependencies",
                dependencies
                    .iter()
                    .map(|dependency| serde_json::Value::from(dependency.to_string()))
                    .collect(),
            );
        }
        if !outputs.is_empty() {
            insert("outputs", outputs.into());
        }
        if !names.is_empty() {
            insert(
                "source_properties",
                names
                    .into_iter()
                    .zip(values)
                    .map(|(name, value)| (name, serde_json::Value::from(value)))
                    .collect::<serde_json::Map<_, _>>()
                    .into(),
            );
        }
        if let Some(definition) = product_definition {
            insert("product_definition", definition.to_string().into());
        }
        let feature: Feature = serde_json::from_value(serde_json::Value::Object(wire))
            .map_err(|error| format!("the feature does not deserialize: {error}"))?;
        let operation = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. }
            | FeatureDefinition::Operation(operation) => operation,
        };
        if matches!(operation, FeatureOperation::HelixNativeAxis { .. }) {
            return Err(
                "the helix axis names a native record, which a stage does not hold".to_owned(),
            );
        }
        let topologies = |builder: &mut Self, name: &str| {
            token(prim, name, &mut builder.issues)
                .map(|text| serde_json::from_str::<serde_json::Value>(&text))
                .transpose()
                .map_err(|error| format!("{name}: {error}"))
        };
        let inputs: Vec<FeatureInputTopology> =
            match topologies(self, "cad:feature:inputTopologies")? {
                Some(value) => serde_json::from_value(value)
                    .map_err(|error| format!("cad:feature:inputTopologies: {error}"))?,
                None => Vec::new(),
            };
        let results: Vec<FeatureResultTopology> =
            match topologies(self, "cad:feature:resultTopologies")? {
                Some(value) => serde_json::from_value(value)
                    .map_err(|error| format!("cad:feature:resultTopologies: {error}"))?,
                None => Vec::new(),
            };
        if inputs
            .iter()
            .any(|topology| topology.input_of != feature.id)
            || results
                .iter()
                .any(|topology| topology.output_of != feature.id)
        {
            return Err("a topology record names another feature".to_owned());
        }
        Ok(Read {
            feature,
            parent,
            inputs,
            results,
            references,
        })
    }

    /// The product definition that relationship `name` of `prim` targets.
    pub(super) fn definition_target(
        &mut self,
        prim: &Prim<'_>,
        name: &str,
    ) -> Result<Option<cadmpeg_ir::ids::ProductDefinitionId>, String> {
        match prim.relationship_targets(name).as_slice() {
            [] => Ok(None),
            [target] => {
                let stage = self.stage;
                let holder = stage
                    .find(target)
                    .ok_or_else(|| format!("{name}: <{target}> is not a prim"))?;
                let key = self.definition_key(holder);
                let definition = self.definition_id(&key);
                if self
                    .ir
                    .model
                    .product_definitions
                    .iter()
                    .any(|candidate| candidate.id == definition)
                {
                    Ok(Some(definition))
                } else {
                    Err(format!("{name}: <{target}> defines no product definition"))
                }
            }
            _ => Err(format!("{name} has more than one target")),
        }
    }
}
