// SPDX-License-Identifier: Apache-2.0
//! Resolvers for independent display and placement facts.

use std::collections::HashSet;

use super::CadIr;
use crate::appearance::AppearanceTarget;
use crate::ids::BodyId;
use crate::presentation::{PresentationItem, PresentationLayer};
use crate::products::{OccurrenceParent, PrototypeReference};
use crate::topology::Color;

impl CadIr {
    /// Resolves display colour, falling back to material colour only when no
    /// display colour is set. An explicit face assignment precedes body fallback;
    /// an occurrence assignment precedes its single-body prototype fallback.
    pub fn effective_display_color(&self, target: &AppearanceTarget) -> Option<Color> {
        let typed_target = target.typed_record_target(&self.model);
        let target = typed_target.as_ref().unwrap_or(target);
        let display = match target {
            AppearanceTarget::Body(id) => self
                .model
                .bodies
                .iter()
                .find(|body| body.id == *id)
                .and_then(|body| body.color),
            AppearanceTarget::Face(id) => self
                .model
                .faces
                .iter()
                .find(|face| face.id == *id)
                .and_then(|face| face.color),
            _ => None,
        };
        display
            .or_else(|| self.bound_color(target))
            .or_else(|| match target {
                AppearanceTarget::Body(_) => None,
                AppearanceTarget::Occurrence(id) => {
                    let occurrence = self
                        .model
                        .occurrences
                        .iter()
                        .find(|record| record.id == *id)?;
                    let PrototypeReference::Local { definition } = &occurrence.prototype else {
                        return None;
                    };
                    let prototype = self
                        .model
                        .product_definitions
                        .iter()
                        .find(|record| record.id == *definition)?;
                    let [body] = prototype.bodies.as_slice() else {
                        return None;
                    };
                    self.effective_display_color(&AppearanceTarget::Body(body.clone()))
                }
                _ => self.target_body(target).and_then(|body| {
                    self.effective_display_color(&AppearanceTarget::Body(body.clone()))
                }),
            })
    }

    fn bound_color(&self, target: &AppearanceTarget) -> Option<Color> {
        self.model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                binding
                    .target
                    .typed_record_target(&self.model)
                    .as_ref()
                    .unwrap_or(&binding.target)
                    == target
            })
            .find_map(|binding| {
                self.model
                    .appearances
                    .iter()
                    .find(|appearance| appearance.id == binding.appearance)
                    .and_then(|appearance| appearance.base_color)
            })
    }

    /// Combines applicable explicit visibility gates with logical AND. Absence
    /// of all gates returns `None`. Object, binding, view and layer visibility
    /// remain independent facts; a hidden containing layer or occurrence hides
    /// its descendants. A body query never combines its different instances.
    pub fn effective_visibility(&self, target: &AppearanceTarget) -> Option<bool> {
        let typed_target = target.typed_record_target(&self.model);
        let target = typed_target.as_ref().unwrap_or(target);
        let mut answer = None;
        let mut targets = vec![target.clone()];
        if let Some(body) = self.target_body(target) {
            let body_target = AppearanceTarget::Body(body.clone());
            if body_target != *target {
                targets.push(body_target);
            }
        }
        if let AppearanceTarget::Occurrence(id) = target {
            let mut cursor = Some(id);
            let mut seen = HashSet::new();
            while let Some(id) = cursor {
                if !seen.insert(id.as_str()) {
                    break;
                }
                let Some(occurrence) = self
                    .model
                    .occurrences
                    .iter()
                    .find(|record| record.id == *id)
                else {
                    break;
                };
                gate(&mut answer, occurrence.visible);
                let parent_target = AppearanceTarget::Occurrence(id.clone());
                if parent_target != *target {
                    targets.push(parent_target);
                }
                if let PrototypeReference::Local { definition } = &occurrence.prototype {
                    let item = PresentationItem::Product {
                        product: definition.clone(),
                    };
                    for layer in self
                        .model
                        .presentation_layers
                        .iter()
                        .filter(|layer| layer.items.contains(&item))
                    {
                        self.layer_visibility(layer, &mut answer);
                    }
                    if let Some(prototype) = self
                        .model
                        .product_definitions
                        .iter()
                        .find(|record| record.id == *definition)
                    {
                        for view in self.model.view_presentations.iter().filter(|view| {
                            view.object.as_deref() == Some(definition.as_str())
                                || prototype
                                    .native_ref
                                    .as_deref()
                                    .is_some_and(|native| view.object.as_deref() == Some(native))
                        }) {
                            gate(&mut answer, view.visible);
                        }
                    }
                }
                cursor = match &occurrence.parent {
                    OccurrenceParent::Root {} => None,
                    OccurrenceParent::Occurrence { occurrence } => Some(occurrence),
                };
            }
        }
        for target in &targets {
            if let AppearanceTarget::Body(id) = target {
                gate(
                    &mut answer,
                    self.model
                        .bodies
                        .iter()
                        .find(|body| body.id == *id)
                        .and_then(|body| body.visible),
                );
            }
            for binding in self.model.appearance_bindings.iter().filter(|binding| {
                binding
                    .target
                    .typed_record_target(&self.model)
                    .as_ref()
                    .unwrap_or(&binding.target)
                    == target
            }) {
                gate(&mut answer, binding.visible);
            }
            let identities = self.target_objects(target);
            for view in self.model.view_presentations.iter().filter(|view| {
                view.object
                    .as_deref()
                    .is_some_and(|object| identities.contains(object))
            }) {
                gate(&mut answer, view.visible);
            }
            for layer in &self.model.presentation_layers {
                if layer.items.iter().any(|item| target_matches(item, target) || matches!(item, PresentationItem::Source {source_id} if identities.contains(source_id.as_str()))) {
                    self.layer_visibility(layer, &mut answer);
                }
            }
        }
        answer
    }

    fn layer_visibility(&self, layer: &PresentationLayer, answer: &mut Option<bool>) {
        let mut current = Some(layer);
        let mut seen = HashSet::new();
        while let Some(layer) = current {
            if !seen.insert(layer.id.as_str()) {
                break;
            }
            gate(answer, layer.visible);
            current = layer.parent.as_ref().and_then(|id| {
                self.model
                    .presentation_layers
                    .iter()
                    .find(|record| record.id == *id)
            });
        }
    }

    fn target_body(&self, target: &AppearanceTarget) -> Option<&BodyId> {
        if let AppearanceTarget::Body(id) = target {
            return self
                .model
                .bodies
                .iter()
                .find(|record| record.id == *id)
                .map(|record| &record.id);
        }
        if let AppearanceTarget::Tessellation(id) = target {
            return self
                .model
                .tessellations
                .iter()
                .find(|record| record.id.as_str() == id)
                .and_then(|record| record.body.as_ref());
        }
        if let AppearanceTarget::TriangleGroup { tessellation, .. } = target {
            return self
                .model
                .tessellations
                .iter()
                .find(|record| record.id.as_str() == tessellation)
                .and_then(|record| record.body.as_ref());
        }
        let shell = match target {
            AppearanceTarget::Face(id) => self
                .model
                .faces
                .iter()
                .find(|record| record.id == *id)
                .map(|record| &record.shell),
            AppearanceTarget::Edge(id) => self
                .model
                .shells
                .iter()
                .find(|shell| shell.wire_edges().contains(id))
                .map(|shell| &shell.id)
                .or_else(|| {
                    let coedge = self
                        .model
                        .coedges
                        .iter()
                        .find(|coedge| coedge.edge == *id)?;
                    let loop_ = self
                        .model
                        .loops
                        .iter()
                        .find(|loop_| loop_.id == coedge.owner_loop)?;
                    self.model
                        .faces
                        .iter()
                        .find(|face| face.id == loop_.face)
                        .map(|face| &face.shell)
                }),
            AppearanceTarget::Vertex(id) => self
                .model
                .shells
                .iter()
                .find(|shell| shell.free_vertices().contains(id))
                .map(|shell| &shell.id),
            _ => None,
        }?;
        let shell = self
            .model
            .shells
            .iter()
            .find(|record| record.id == *shell)?;
        self.model
            .regions
            .iter()
            .find(|record| record.id == shell.region)
            .map(|record| &record.body)
    }

    fn target_objects<'a>(&'a self, target: &'a AppearanceTarget) -> HashSet<&'a str> {
        let mut ids = HashSet::new();
        match target {
            AppearanceTarget::Body(id) => {
                ids.insert(id.as_str());
                for definition in &self.model.product_definitions {
                    if definition.bodies.contains(id) {
                        if let Some(native) = &definition.native_ref {
                            ids.insert(native.as_str());
                        }
                    }
                }
            }
            AppearanceTarget::Face(id) => {
                ids.insert(id.as_str());
            }
            AppearanceTarget::Edge(id) => {
                ids.insert(id.as_str());
            }
            AppearanceTarget::Vertex(id) => {
                ids.insert(id.as_str());
            }
            AppearanceTarget::Curve(id) => {
                ids.insert(id.as_str());
                if let Some(source) = self
                    .model
                    .curves
                    .iter()
                    .find(|record| record.id == *id)
                    .and_then(|record| record.source_object.as_ref())
                {
                    ids.insert(source.object_id.as_str());
                }
            }
            AppearanceTarget::Surface(id) => {
                ids.insert(id.as_str());
                if let Some(source) = self
                    .model
                    .surfaces
                    .iter()
                    .find(|record| record.id == *id)
                    .and_then(|record| record.source_object.as_ref())
                {
                    ids.insert(source.object_id.as_str());
                }
            }
            AppearanceTarget::Point(id) => {
                ids.insert(id.as_str());
                if let Some(source) = self
                    .model
                    .points
                    .iter()
                    .find(|record| record.id == *id)
                    .and_then(|record| record.source_object.as_ref())
                {
                    ids.insert(source.object_id.as_str());
                }
            }
            AppearanceTarget::Tessellation(id) => {
                ids.insert(id.as_str());
                if let Some(source) = self
                    .model
                    .tessellations
                    .iter()
                    .find(|record| record.id.as_str() == id)
                    .and_then(|record| record.source_object.as_ref())
                {
                    ids.insert(source.object_id.as_str());
                }
            }
            AppearanceTarget::TriangleGroup { tessellation, group } => {
                ids.insert(tessellation.as_str());
                ids.insert(group.as_str());
                if let Some(source) = self
                    .model
                    .tessellations
                    .iter()
                    .find(|record| record.id.as_str() == tessellation)
                    .and_then(|record| record.source_object.as_ref())
                {
                    ids.insert(source.object_id.as_str());
                }
            }
            AppearanceTarget::Occurrence(id) => {
                ids.insert(id.as_str());
                if let Some(native) = self
                    .model
                    .occurrences
                    .iter()
                    .find(|record| record.id == *id)
                    .and_then(|record| record.native_ref.as_ref())
                {
                    ids.insert(native.as_str());
                }
            }
            AppearanceTarget::Record { arena, id } => {
                if self.model.contains_record(*arena, id) {
                    ids.insert(id.as_str());
                }
            }
            AppearanceTarget::Source { source_id } => {
                ids.insert(source_id.as_str());
            }
        }
        ids
    }
}

fn gate(answer: &mut Option<bool>, value: Option<bool>) {
    if let Some(value) = value {
        *answer = Some(answer.unwrap_or(true) && value);
    }
}

fn target_matches(item: &PresentationItem, target: &AppearanceTarget) -> bool {
    match (item, target) {
        (PresentationItem::Body { body }, AppearanceTarget::Body(id)) => body == id,
        (PresentationItem::Face { face }, AppearanceTarget::Face(id)) => face == id,
        (PresentationItem::Edge { edge }, AppearanceTarget::Edge(id)) => edge == id,
        (PresentationItem::Vertex { vertex }, AppearanceTarget::Vertex(id)) => vertex == id,
        (PresentationItem::Curve { curve }, AppearanceTarget::Curve(id)) => curve == id,
        (PresentationItem::Surface { surface }, AppearanceTarget::Surface(id)) => surface == id,
        (PresentationItem::Point { point }, AppearanceTarget::Point(id)) => point == id,
        (PresentationItem::Occurrence { occurrence }, AppearanceTarget::Occurrence(id)) => {
            occurrence == id
        }
        (PresentationItem::Tessellation { tessellation }, AppearanceTarget::Tessellation(id)) => {
            tessellation == id
        }
        (
            PresentationItem::Source { source_id: source },
            AppearanceTarget::Source { source_id },
        ) => source.as_str() == source_id,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
