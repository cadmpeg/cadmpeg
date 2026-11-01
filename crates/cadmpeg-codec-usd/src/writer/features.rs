// SPDX-License-Identifier: Apache-2.0
//! Writes construction features as `CadFeature` prims (spec §18.7).

use cadmpeg_ir::features::Feature;

use super::text::{quoted, Names};
use super::Writer;

/// Name of the scope that holds the features.
pub(super) const FEATURES: &str = "Features";

/// Fields of the wire form of a feature that typed attributes state.
const TYPED_FIELDS: &[&str] = &[
    "product_definition",
    "id",
    "ordinal",
    "name",
    "suppressed",
    "dependencies",
    "source_properties",
    "outputs",
    "native_ref",
];

impl Writer<'_> {
    /// Writes the `Features` scope, after naming the prim of each feature so
    /// that a parameter, a configuration, or another feature names it.
    pub(super) fn features(&mut self, names: &mut Names) {
        if self.ir.model.features.is_empty() {
            return;
        }
        names.reserve(FEATURES);
        self.prim_head("def Scope", FEATURES, &[]);
        self.out.open();
        let scope = self.out.path();
        let mut feature_names = Names::default();
        for feature in &self.ir.model.features {
            let name = feature_names.unique(feature.name.as_deref().unwrap_or("feature"));
            self.feature_paths
                .insert(feature.id.clone(), format!("{scope}/{name}"));
        }
        for feature in &self.ir.model.features {
            self.feature(feature);
        }
        self.out.close();
    }

    fn feature(&mut self, feature: &Feature) {
        let Some(path) = self.feature_paths.get(&feature.id) else {
            return;
        };
        let Ok(serde_json::Value::Object(mut payload)) = serde_json::to_value(feature) else {
            self.unwritten.features += 1;
            return;
        };
        for field in TYPED_FIELDS {
            payload.remove(*field);
        }
        let operation = payload
            .get("definition")
            .and_then(|definition| definition.get("definition"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let name = path.rsplit('/').next().unwrap_or(FEATURES).to_owned();
        self.prim_head("def CadFeature", &name, &[]);
        self.out.open();
        let mut lines = vec![format!(
            "uniform string cad:id = {}",
            quoted(feature.id.as_str())
        )];
        if let Some(name) = &feature.name {
            lines.push(format!(
                "uniform string cad:feature:name = {}",
                quoted(name)
            ));
        }
        lines.push(format!(
            "uniform uint64 cad:feature:ordinal = {}",
            feature.ordinal
        ));
        if let Some(suppressed) = feature.suppressed {
            lines.push(format!(
                "uniform bool cad:feature:suppressed = {}",
                u8::from(suppressed)
            ));
        }
        lines.push(format!(
            "uniform token cad:feature:operation = {}",
            quoted(&operation)
        ));
        let dependencies = self.feature_targets(feature.dependencies.iter());
        if !dependencies.is_empty() {
            lines.push(format!(
                "rel cad:feature:dependencies = [{}]",
                dependencies.join(", ")
            ));
        }
        if let Some(parent) = self
            .ir
            .model
            .feature_regeneration_parent(&feature.id)
            .and_then(|parent| self.feature_paths.get(parent))
        {
            lines.push(format!("rel cad:feature:regenerationParent = <{parent}>"));
        }
        if let Some(path) = feature
            .product_definition
            .as_ref()
            .and_then(|definition| self.definition_path(definition))
        {
            lines.push(format!("rel cad:feature:definition = <{path}>"));
        }
        for line in lines {
            self.out.line(&line);
        }
        let outputs: Vec<String> = feature
            .evaluation
            .outputs()
            .iter()
            .map(|body| quoted(body.as_str()))
            .collect();
        if !outputs.is_empty() {
            self.out
                .array("uniform string[] cad:feature:outputs", &outputs, "");
        }
        if !feature.source_properties.is_empty() {
            let keys: Vec<String> = feature
                .source_properties
                .keys()
                .map(|key| quoted(key.as_str()))
                .collect();
            let values: Vec<String> = feature
                .source_properties
                .values()
                .map(|value| quoted(value))
                .collect();
            self.out
                .array("uniform string[] cad:feature:propertyNames", &keys, "");
            self.out
                .array("uniform string[] cad:feature:propertyValues", &values, "");
        }
        self.out.line(&format!(
            "uniform string cad:feature:payload = {}",
            quoted(&serde_json::Value::Object(payload).to_string())
        ));
        let model = &self.ir.model;
        let inputs: Vec<_> = model
            .feature_input_topologies
            .iter()
            .filter(|topology| topology.input_of == feature.id)
            .collect();
        if let Some(text) = json_array(&inputs) {
            self.out.line(&format!(
                "uniform string cad:feature:inputTopologies = {}",
                quoted(&text)
            ));
        }
        let results: Vec<_> = model
            .feature_result_topologies
            .iter()
            .filter(|topology| topology.output_of == feature.id)
            .collect();
        if let Some(text) = json_array(&results) {
            self.out.line(&format!(
                "uniform string cad:feature:resultTopologies = {}",
                quoted(&text)
            ));
        }
        self.out.close();
    }

    /// The relationship targets of the `CadFeature` prims of `features`.
    fn feature_targets<'f>(
        &self,
        features: impl Iterator<Item = &'f cadmpeg_ir::features::FeatureId>,
    ) -> Vec<String> {
        features
            .filter_map(|feature| self.feature_paths.get(feature))
            .map(|path| format!("<{path}>"))
            .collect()
    }

    /// The `CadFeature` prim path of `feature`.
    pub(super) fn feature_path(&self, feature: &cadmpeg_ir::features::FeatureId) -> Option<&str> {
        self.feature_paths.get(feature).map(String::as_str)
    }
}

/// `records` as a JSON array, or `None` when there are none.
fn json_array<T: serde::Serialize>(records: &[T]) -> Option<String> {
    if records.is_empty() {
        return None;
    }
    serde_json::to_string(records).ok()
}
