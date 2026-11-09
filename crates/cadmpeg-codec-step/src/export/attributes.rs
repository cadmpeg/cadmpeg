// SPDX-License-Identifier: Apache-2.0
//! Source attributes written as STEP property definitions.

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

use super::Builder;
use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::writer::{refs, string, Ref};

impl Builder<'_> {
    pub(super) fn emit_document_attributes(&mut self, owner: Ref, context: Ref) {
        if !matches!(
            self.schema,
            StepSchema::Ap242Edition1 | StepSchema::Ap242Edition2 | StepSchema::Ap242Edition3
        ) {
            return;
        }
        let ir = self.ir;
        for attribute in &ir.model.attributes {
            if attribute.target != AttributeTarget::Document || self.attribute_is_written(attribute)
            {
                continue;
            }
            let mut values = Vec::new();
            for value in &attribute.values {
                let held = match value {
                    Some(AttributeValue::String(value)) => {
                        Some(("DESCRIPTIVE_REPRESENTATION_ITEM", string(value)))
                    }
                    Some(AttributeValue::Integer(value)) => Some((
                        "VALUE_REPRESENTATION_ITEM",
                        format!("COUNT_MEASURE({value})"),
                    )),
                    Some(AttributeValue::Float(value)) => Some((
                        "VALUE_REPRESENTATION_ITEM",
                        format!("NUMERIC_MEASURE({})", self.emitter.real(value.get())),
                    )),
                    Some(AttributeValue::Boolean(value)) => Some((
                        "BOOLEAN_REPRESENTATION_ITEM",
                        if *value { ".T." } else { ".F." }.to_owned(),
                    )),
                    _ => None,
                };
                values.extend(held);
            }
            if values.is_empty() || values.len() != attribute.values.len() {
                self.loss(
                    StepLossCode::DocumentAttributeValueDegraded,
                    format!(
                        "document attribute '{}' value cannot be held in the CAx-IF representation form",
                        attribute.name
                    ),
                );
                continue;
            }
            let name = string(&attribute.name);
            let property = self
                .emitter
                .emit("PROPERTY_DEFINITION", &format!("{name},'',{owner}"));
            // Several values are the ordered row the attribute name labels.
            let item = if let [(kind, value)] = values.as_slice() {
                self.emitter.emit(kind, &format!("{name},{value}"))
            } else {
                let items: Vec<_> = values
                    .iter()
                    .map(|(kind, value)| self.emitter.emit(kind, &format!("'',{value}")))
                    .collect();
                self.emitter.emit(
                    "ROW_REPRESENTATION_ITEM",
                    &format!("{name},{}", refs(&items)),
                )
            };
            let representation = self
                .emitter
                .emit("REPRESENTATION", &format!("'',({item}),{context}"));
            self.emitter.emit(
                "PROPERTY_DEFINITION_REPRESENTATION",
                &format!("{property},{representation}"),
            );
            self.consumed_attribute_ids
                .insert(attribute.id.as_str().to_owned());
        }
    }

    pub(super) fn emit_scalar_attribute(
        &mut self,
        attribute: &cadmpeg_ir::attributes::SourceAttribute,
        owner: Ref,
        context: Ref,
    ) {
        let scalar_items = !matches!(self.schema, StepSchema::Ap203Edition1 | StepSchema::Ap214);
        let mut values = Vec::new();
        for value in &attribute.values {
            let item = match value {
                Some(AttributeValue::String(value)) => {
                    ("DESCRIPTIVE_REPRESENTATION_ITEM", string(value))
                }
                Some(AttributeValue::Integer(value)) if scalar_items => {
                    ("INTEGER_REPRESENTATION_ITEM", value.to_string())
                }
                Some(AttributeValue::Float(value)) if scalar_items => {
                    ("REAL_REPRESENTATION_ITEM", self.emitter.real(value.get()))
                }
                Some(AttributeValue::Boolean(value)) if scalar_items => (
                    "BOOLEAN_REPRESENTATION_ITEM",
                    if *value { ".T." } else { ".F." }.to_owned(),
                ),
                _ => return,
            };
            values.push(item);
        }
        if values.is_empty() || (values.len() > 1 && !scalar_items) {
            return;
        }
        let property = self.emitter.emit(
            "PROPERTY_DEFINITION",
            &format!("{},'',{owner}", string(&attribute.name)),
        );
        let items: Vec<_> = values
            .into_iter()
            .map(|(kind, value)| self.emitter.emit(kind, &format!("'',{value}")))
            .collect();
        let item = if let [item] = items.as_slice() {
            *item
        } else {
            self.emitter
                .emit("ROW_REPRESENTATION_ITEM", &format!("'',{}", refs(&items)))
        };
        let representation = self
            .emitter
            .emit("REPRESENTATION", &format!("'',({item}),{context}"));
        self.emitter.emit(
            "PROPERTY_DEFINITION_REPRESENTATION",
            &format!("{property},{representation}"),
        );
        if matches!(attribute.target, AttributeTarget::Record { .. }) {
            self.loss(
                StepLossCode::SourceAttributeTargetNormalized,
                format!("attribute {} was written with a typed target", attribute.id),
            );
        }
        self.consumed_attribute_ids
            .insert(attribute.id.as_str().to_owned());
    }

    pub(super) fn emit_geometry_attributes(&mut self, context: Ref) {
        if !matches!(
            self.schema,
            StepSchema::Ap242Edition1 | StepSchema::Ap242Edition2 | StepSchema::Ap242Edition3
        ) {
            return;
        }
        let ir = self.ir;
        for attribute in &ir.model.attributes {
            if self.attribute_is_written(attribute)
                || attribute.values.is_empty()
                || !attribute.values.iter().all(|value| {
                    matches!(
                        value,
                        Some(
                            AttributeValue::String(_)
                                | AttributeValue::Integer(_)
                                | AttributeValue::Float(_)
                                | AttributeValue::Boolean(_)
                        )
                    )
                })
            {
                continue;
            }
            let item = match &attribute.target {
                AttributeTarget::Body(id) => {
                    self.body_step_item_refs.get(id.as_str()).and_then(|items| {
                        match items.as_slice() {
                            [item] => Some(*item),
                            _ => None,
                        }
                    })
                }
                AttributeTarget::Face(id) => self.face_step_refs.get(id.as_str()).copied(),
                AttributeTarget::Edge(id) if !self.split_edges.contains(id.as_str()) => {
                    self.edge_refs.get(id.as_str()).copied()
                }
                AttributeTarget::Vertex(id) => self.vertex_refs.get(id.as_str()).copied(),
                AttributeTarget::Surface(id) => self
                    .surface_refs
                    .get(id.as_str())
                    .map(|surface| surface.reference),
                AttributeTarget::Curve(id) => self.curve_refs.get(id.as_str()).copied(),
                AttributeTarget::Point(id) => self.point_refs.get(id.as_str()).copied(),
                AttributeTarget::Tessellation(id) => self.tessellation_step_refs.get(id).copied(),
                AttributeTarget::Record { arena, id } => match arena.as_str() {
                    "bodies" => {
                        self.body_step_item_refs
                            .get(id)
                            .and_then(|items| match items.as_slice() {
                                [item] => Some(*item),
                                _ => None,
                            })
                    }
                    "faces" => self.face_step_refs.get(id).copied(),
                    "edges" if !self.split_edges.contains(id.as_str()) => {
                        self.edge_refs.get(id).copied()
                    }
                    "vertices" => self.vertex_refs.get(id).copied(),
                    "surfaces" => self.surface_refs.get(id).map(|surface| surface.reference),
                    "curves" => self.curve_refs.get(id).copied(),
                    "points" => self.point_refs.get(id).copied(),
                    "tessellations" => self.tessellation_step_refs.get(id).copied(),
                    _ => None,
                },
                _ => None,
            };
            let Some(item) = item else {
                continue;
            };
            let representation = self
                .emitter
                .emit("REPRESENTATION", &format!("'',({item}),{context}"));
            let owner = self.emitter.emit(
                "CHARACTERIZED_ITEM_WITHIN_REPRESENTATION",
                &format!("'','',{item},{representation}"),
            );
            self.emit_scalar_attribute(attribute, owner, context);
        }
    }

    pub(super) fn emit_occurrence_attributes(&mut self, context: Ref) {
        let ir = self.ir;
        for attribute in &ir.model.attributes {
            let occurrence = match &attribute.target {
                AttributeTarget::Occurrence(id) => self.occurrence_step_refs.get(id.as_str()),
                AttributeTarget::Record { arena, id } if arena.as_str() == "occurrences" => {
                    self.occurrence_step_refs.get(id)
                }
                _ => None,
            };
            if let Some(usage) = occurrence.copied() {
                self.emit_scalar_attribute(attribute, usage, context);
            }
        }
    }
}
