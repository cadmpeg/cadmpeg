// SPDX-License-Identifier: Apache-2.0
//! Writes presentation layers as `CadLayer` prims and source attributes as
//! one `CadAttributes` prim (spec §18.10 and §18.11).

use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

use super::text::{quoted, Names};
use super::Writer;

/// Name of the scope that holds the layers and attributes.
pub(super) const PRESENTATION: &str = "Presentation";

impl Writer<'_> {
    /// Writes the `Presentation` scope.
    pub(super) fn presentation(&mut self, names: &mut Names) {
        let model = &self.ir.model;
        if model.presentation_layers.is_empty() && model.attributes.is_empty() {
            return;
        }
        names.reserve(PRESENTATION);
        self.prim_head("def Scope", PRESENTATION, &[]);
        self.out.open();
        let mut inner = Names::default();
        for layer in &model.presentation_layers {
            let name = inner.unique(&layer.name);
            self.presentation_layer(layer, &name);
        }
        if !model.attributes.is_empty() {
            let name = inner.unique("Attributes");
            self.attributes(&name);
        }
        self.out.close();
    }

    fn presentation_layer(&mut self, layer: &PresentationLayer, name: &str) {
        let Ok(items) = serde_json::to_string(&layer.items) else {
            self.unwritten.presentation += 1;
            return;
        };
        let members: Vec<String> = layer
            .items
            .iter()
            .filter_map(|item| match item {
                PresentationItem::Occurrence { occurrence } => self.paths.get(occurrence).cloned(),
                PresentationItem::Product { product } => {
                    self.definition_path(product).map(str::to_owned)
                }
                _ => None,
            })
            .map(|path| format!("<{path}>"))
            .collect();
        let metadata = if members.is_empty() {
            Vec::new()
        } else {
            vec!["prepend apiSchemas = [\"CollectionAPI:members\"]".to_owned()]
        };
        self.prim_head("def CadLayer", name, &metadata);
        self.out.open();
        self.out.line(&format!(
            "uniform string cad:id = {}",
            quoted(layer.id.as_str())
        ));
        self.out.line(&format!(
            "uniform string cad:layer:name = {}",
            quoted(&layer.name)
        ));
        if let Some(description) = &layer.description {
            self.out.line(&format!(
                "uniform string cad:layer:description = {}",
                quoted(description)
            ));
        }
        if let Some(visible) = layer.visible {
            self.out.line(&format!(
                "uniform bool cad:layer:visible = {}",
                u8::from(visible)
            ));
        }
        self.out.line(&format!(
            "uniform string cad:layer:items = {}",
            quoted(&items)
        ));
        if !members.is_empty() {
            self.out
                .line("uniform token collection:members:expansionRule = \"expandPrims\"");
            self.out.line(&format!(
                "rel collection:members:includes = [{}]",
                members.join(", ")
            ));
        }
        self.out.close();
    }

    fn attributes(&mut self, name: &str) {
        let model = &self.ir.model;
        let mut ids = Vec::new();
        let mut targets = Vec::new();
        let mut names = Vec::new();
        let mut values = Vec::new();
        for attribute in &model.attributes {
            let (Ok(target), Ok(value)) = (
                serde_json::to_string(&attribute.target),
                serde_json::to_string(&attribute.values),
            ) else {
                self.unwritten.presentation += 1;
                continue;
            };
            ids.push(quoted(attribute.id.as_str()));
            targets.push(quoted(&target));
            names.push(quoted(&attribute.name));
            values.push(quoted(&value));
        }
        self.prim_head("def CadAttributes", name, &[]);
        self.out.open();
        self.out
            .array("uniform string[] cad:attribute:ids", &ids, "");
        self.out
            .array("uniform string[] cad:attribute:targets", &targets, "");
        self.out
            .array("uniform string[] cad:attribute:names", &names, "");
        self.out
            .array("uniform string[] cad:attribute:values", &values, "");
        self.out.close();
    }
}
