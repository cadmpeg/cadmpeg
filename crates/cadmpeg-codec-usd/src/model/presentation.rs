// SPDX-License-Identifier: Apache-2.0
//! Presentation layers and source attributes from `CadLayer` and
//! `CadAttributes` prims (spec §18.10 and §18.11).

use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
use cadmpeg_ir::ids::LayerId;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

use crate::schema::{boolean, token, tokens};
use crate::stage::Prim;

/// The type of a `CadLayer` prim.
pub(super) const LAYER_TYPE: &str = "CadLayer";
/// The type of the `CadAttributes` prim.
pub(super) const ATTRIBUTES_TYPE: &str = "CadAttributes";

/// Whether a prim type is `CadLayer` or `CadAttributes`.
pub(super) fn is_presentation_type(type_name: Option<&str>) -> bool {
    matches!(type_name, Some(LAYER_TYPE | ATTRIBUTES_TYPE))
}

/// The records read from the presentation prims, before their references
/// are checked.
#[derive(Default)]
struct Read {
    layers: Vec<(String, PresentationLayer)>,
    attributes: Vec<(String, SourceAttribute)>,
}

impl super::Builder<'_, '_, '_, '_> {
    /// Adds the layers and attributes of the stage, after every record that
    /// they name.
    pub(super) fn presentation(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let read = self.read_presentation();
        let mut dropped = Vec::new();
        let (layers, attributes) = {
            let index = ModelIndex::new_model_only(self.ir);
            let held = |id: &str| index.contains(id, StandardIndex);
            let mut layers = Vec::new();
            for (path, mut layer) in read.layers {
                let listed = layer.items.len();
                layer.items.retain(|item| match item {
                    PresentationItem::Body { body } => held(body.as_str()),
                    PresentationItem::Face { face } => held(face.as_str()),
                    PresentationItem::Edge { edge } => held(edge.as_str()),
                    PresentationItem::Vertex { vertex } => held(vertex.as_str()),
                    PresentationItem::Point { point } => held(point.as_str()),
                    PresentationItem::Curve { curve } => held(curve.as_str()),
                    PresentationItem::Surface { surface } => held(surface.as_str()),
                    PresentationItem::Product { product } => held(product.as_str()),
                    PresentationItem::Occurrence { occurrence } => held(occurrence.as_str()),
                    PresentationItem::Pmi { annotation } => held(annotation.as_str()),
                    PresentationItem::Tessellation { tessellation } => held(tessellation),
                    PresentationItem::Source { .. } => true,
                });
                if layer.items.len() < listed {
                    dropped.push(format!("{path}: {} items", listed - layer.items.len()));
                }
                layers.push(layer);
            }
            let mut attributes = Vec::new();
            for (path, attribute) in read.attributes {
                let target = match &attribute.target {
                    AttributeTarget::Document => true,
                    AttributeTarget::Body(id) => held(id.as_str()),
                    AttributeTarget::Face(id) => held(id.as_str()),
                    AttributeTarget::Shell(id) => held(id.as_str()),
                    AttributeTarget::Loop(id) => held(id.as_str()),
                    AttributeTarget::Coedge(id) => held(id.as_str()),
                    AttributeTarget::Edge(id) => held(id.as_str()),
                    AttributeTarget::Vertex(id) => held(id.as_str()),
                };
                if target {
                    attributes.push(attribute);
                } else {
                    dropped.push(format!("{path}: attribute {}", attribute.id));
                }
            }
            (layers, attributes)
        };
        self.tallies.presentation_properties.extend(dropped);
        for layer in layers {
            self.ir.model.presentation_layers.push(layer);
            self.admit("usd_layer")?;
        }
        for attribute in attributes {
            self.ir.model.attributes.push(attribute);
            self.admit("usd_attribute")?;
        }
        Ok(())
    }

    fn read_presentation(&mut self) -> Read {
        let stage = self.stage;
        let mut read = Read::default();
        for prim in stage
            .prims
            .iter()
            .filter(|prim| is_presentation_type(prim.type_name()))
        {
            let result = match prim.type_name() {
                Some(ATTRIBUTES_TYPE) => self.attribute_records(prim).map(|records| {
                    read.attributes.extend(
                        records
                            .into_iter()
                            .map(|record| (prim.path.clone(), record)),
                    );
                }),
                _ => self
                    .layer(prim)
                    .map(|layer| read.layers.push((prim.path.clone(), layer))),
            };
            if let Err(reason) = result {
                self.tallies
                    .rejected_presentation
                    .push(format!("{}: {reason}", prim.path));
            }
        }
        read
    }

    fn layer(&mut self, prim: &Prim<'_>) -> Result<PresentationLayer, String> {
        let id = self
            .cad_id(&prim.path, |text| LayerId::mint(text).ok())
            .or_else(|| crate::ids::layer(&prim.path))
            .ok_or("the prim path gives no layer identity")?;
        let items =
            token(prim, "cad:layer:items", &mut self.issues).unwrap_or_else(|| "[]".to_owned());
        let items: Vec<PresentationItem> =
            serde_json::from_str(&items).map_err(|error| format!("cad:layer:items: {error}"))?;
        Ok(PresentationLayer {
            id,
            name: token(prim, "cad:layer:name", &mut self.issues)
                .unwrap_or_else(|| prim.name.clone()),
            description: token(prim, "cad:layer:description", &mut self.issues),
            visible: boolean(prim, "cad:layer:visible", &mut self.issues),
            items,
        })
    }

    fn attribute_records(&mut self, prim: &Prim<'_>) -> Result<Vec<SourceAttribute>, String> {
        let mut array = |name: &str| tokens(prim, name, &mut self.issues).unwrap_or_default();
        let (ids, targets, names, values) = (
            array("cad:attribute:ids"),
            array("cad:attribute:targets"),
            array("cad:attribute:names"),
            array("cad:attribute:values"),
        );
        if [targets.len(), names.len(), values.len()]
            .iter()
            .any(|len| *len != ids.len())
        {
            return Err("the attribute arrays differ in length".to_owned());
        }
        ids.into_iter()
            .zip(targets)
            .zip(names)
            .zip(values)
            .map(|(((id, target), name), values)| {
                let target: serde_json::Value = serde_json::from_str(&target)
                    .map_err(|error| format!("attribute {id}: {error}"))?;
                let values: serde_json::Value = serde_json::from_str(&values)
                    .map_err(|error| format!("attribute {id}: {error}"))?;
                serde_json::from_value(serde_json::json!({
                    "id": id,
                    "target": target,
                    "name": name,
                    "values": values,
                }))
                .map_err(|error| format!("attribute {id}: {error}"))
            })
            .collect()
    }
}
