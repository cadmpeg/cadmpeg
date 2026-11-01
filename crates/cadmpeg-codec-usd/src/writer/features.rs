// SPDX-License-Identifier: Apache-2.0
//! Writes construction features as `CadFeature` prims (spec §18.7).

use cadmpeg_ir::features::Feature;

use super::text::{quoted, real, tuple, Names};
use super::Writer;
use crate::schema::feature::{self, Field, Leaf};

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
        let (prim_type, typed) = payload
            .get_mut("definition")
            .map_or(("CadFeature", Vec::new()), typed_definition);
        let name = path.rsplit('/').next().unwrap_or(FEATURES).to_owned();
        self.prim_head(&format!("def {prim_type}"), &name, &[]);
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
        for line in lines.into_iter().chain(typed) {
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

/// The prim type of `definition` and the attributes of its typed leaves,
/// which it removes from `definition` (spec §18.7). A post-processed
/// operation types the leaves of its operation.
fn typed_definition(definition: &mut serde_json::Value) -> (&'static str, Vec<String>) {
    let tag = |value: &serde_json::Value| {
        value
            .get("definition")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let mut node = definition;
    if tag(node).as_deref() == Some("post_process") {
        match node.get_mut("operation") {
            Some(operation) => node = operation,
            None => return ("CadFeature", Vec::new()),
        }
    }
    let Some(family) = tag(node).as_deref().and_then(feature::family) else {
        return ("CadFeature", Vec::new());
    };
    let mut lines = Vec::new();
    for field in family.fields {
        let name = feature::attribute(family, field);
        if feature::in_array(field) {
            if let Some(line) = typed_array(node, field, &name) {
                lines.push(line);
            }
        } else if let Some(text) = take(node, field.path, field.leaf) {
            lines.push(format!("uniform {} {name} = {text}", usd_type(field.leaf)));
        }
    }
    (family.prim_type, lines)
}

/// The USD type of a leaf attribute.
const fn usd_type(leaf: Leaf) -> &'static str {
    match leaf {
        Leaf::Token => "token",
        Leaf::Text => "string",
        Leaf::Length | Leaf::Angle => "double",
        Leaf::Count => "uint64",
        Leaf::Flag => "bool",
        Leaf::Vector => "vector3d",
        Leaf::Point => "point3d",
        Leaf::Identities => "string[]",
    }
}

/// The attribute text of `value` as `leaf`, when it has that form.
fn leaf_text(value: &serde_json::Value, leaf: Leaf) -> Option<String> {
    let scale = super::MILLIMETRES_PER_UNIT;
    match leaf {
        Leaf::Token | Leaf::Text => value.as_str().filter(|text| !text.is_empty()).map(quoted),
        Leaf::Length => value.as_f64().map(|value| real(value / scale)),
        Leaf::Angle => value.as_f64().map(|value| real(value.to_degrees())),
        Leaf::Count => value.as_u64().map(|value| value.to_string()),
        Leaf::Flag => value.as_bool().map(|flag| u8::from(flag).to_string()),
        Leaf::Identities => {
            let items = value
                .as_array()?
                .iter()
                .map(|item| item.as_str().filter(|text| !text.is_empty()).map(quoted))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[{}]", items.join(", ")))
        }
        Leaf::Vector | Leaf::Point => {
            let map = value.as_object().filter(|map| map.len() == 3)?;
            let unit = if leaf == Leaf::Point { scale } else { 1.0 };
            let axis = |name: &str| map.get(name)?.as_f64().map(|value| value / unit);
            Some(tuple(&[axis("x")?, axis("y")?, axis("z")?]))
        }
    }
}

/// Removes the leaf at `path` of `node` and gives its attribute text, when
/// it has the form of `leaf`. Objects that the removal empties are removed
/// too.
fn take(node: &mut serde_json::Value, path: &[&str], leaf: Leaf) -> Option<String> {
    let (first, rest) = path.split_first()?;
    let map = node.as_object_mut()?;
    if rest.is_empty() {
        let text = leaf_text(map.get(*first)?, leaf)?;
        map.remove(*first);
        return Some(text);
    }
    let child = map.get_mut(*first)?;
    let text = take(child, rest, leaf)?;
    if child.as_object().is_some_and(serde_json::Map::is_empty) {
        map.remove(*first);
    }
    Some(text)
}

/// The array attribute of `field`, whose path holds `*`, when every element
/// that has the leaf has the form of the leaf; it removes those leaves.
fn typed_array(node: &mut serde_json::Value, field: &Field, name: &str) -> Option<String> {
    let star = field.path.iter().position(|segment| *segment == "*")?;
    let (head, tail) = field.path.split_at(star);
    let tail = tail.get(1..)?;
    let mut array = &*node;
    for segment in head {
        array = array.get(*segment)?;
    }
    let elements = array.as_array()?;
    let leaf_of = |element: &serde_json::Value| {
        let mut value = element;
        for segment in tail {
            value = value.get(*segment)?;
        }
        Some(value.clone())
    };
    let values: Vec<Option<serde_json::Value>> = elements.iter().map(leaf_of).collect();
    if values.iter().all(Option::is_none)
        || values
            .iter()
            .flatten()
            .any(|value| leaf_text(value, field.leaf).is_none())
    {
        return None;
    }
    let mut array = &mut *node;
    for segment in head {
        array = array.get_mut(*segment)?;
    }
    for element in array.as_array_mut()? {
        take(element, tail, field.leaf);
    }
    let items: Vec<String> = values
        .iter()
        .map(|value| match value {
            Some(value) => leaf_text(value, field.leaf).unwrap_or_default(),
            None if matches!(field.leaf, Leaf::Token | Leaf::Text) => quoted(""),
            None => real(f64::NAN),
        })
        .collect();
    Some(format!(
        "uniform {}[] {name} = [{}]",
        usd_type(field.leaf),
        items.join(", ")
    ))
}
