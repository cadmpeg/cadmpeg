// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use std::collections::HashSet;

use super::{Check, Finding, ModelIndex, Severity};
use crate::document::CadIr;
use crate::presentation::PresentationItem;

pub(super) fn check_presentation(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    if ir.model.presentation_documents.len() > 1 {
        invalid_state(findings, None, "multiple document presentation records");
    }
    for document in &ir.model.presentation_documents {
        let native_valid = document
            .native_ref
            .as_ref()
            .is_none_or(|native| index.contains(native));
        let assets_valid = document
            .states
            .iter()
            .flat_map(|state| &state.assets)
            .all(|asset| index.contains(asset));
        let orders = document
            .states
            .iter()
            .map(|state| state.order)
            .collect::<HashSet<_>>();
        let camera_valid = document.camera.as_ref().is_none_or(|camera| {
            let finite = camera
                .position
                .iter()
                .flatten()
                .chain(camera.orientation.iter().flatten())
                .all(|value| value.is_finite());
            let quaternion_valid = camera.orientation.is_none_or(|orientation| {
                orientation.iter().map(|value| value * value).sum::<f64>() > f64::EPSILON
            });
            finite && quaternion_valid
        });
        if !native_valid || !assets_valid || orders.len() != document.states.len() || !camera_valid
        {
            invalid_state(
                findings,
                Some(document.id.0.clone()),
                "invalid document presentation state",
            );
        }
    }

    let mut orders = HashSet::new();
    for view in &ir.model.view_presentations {
        let references_valid = view
            .object
            .as_ref()
            .is_none_or(|object| index.contains(object))
            && view
                .native_ref
                .as_ref()
                .is_none_or(|native| index.contains(native));
        let sizes_valid = [view.line_width, view.point_size]
            .into_iter()
            .flatten()
            .all(|value| value.is_finite() && value >= 0.0);
        if !references_valid || !sizes_valid || !orders.insert(view.order) {
            invalid_state(
                findings,
                Some(view.id.0.clone()),
                "invalid view presentation reference, order, or size",
            );
        }
    }

    for layer in &ir.model.presentation_layers {
        if layer.name.is_empty() {
            invalid_layer(
                findings,
                layer.id.as_str(),
                "presentation layer has no name",
            );
        }
        for item in &layer.items {
            let resolved = match item {
                PresentationItem::Body { body } => index.bodies.contains_key(body.as_str()),
                PresentationItem::Face { face } => index.faces.contains_key(face.as_str()),
                PresentationItem::Edge { edge } => index.edges.contains_key(edge.as_str()),
                PresentationItem::Vertex { vertex } => index.vertices.contains_key(vertex.as_str()),
                PresentationItem::Point { point } => index.points.contains_key(point.as_str()),
                PresentationItem::Curve { curve } => index.curves.contains_key(curve.as_str()),
                PresentationItem::Surface { surface } => {
                    index.surfaces.contains_key(surface.as_str())
                }
                PresentationItem::Product { product } => {
                    index.products.contains_key(product.as_str())
                }
                PresentationItem::Occurrence { occurrence } => {
                    index.product_occurrences.contains_key(occurrence.as_str())
                }
                PresentationItem::Pmi { annotation } => index.pmi.contains_key(annotation.as_str()),
                PresentationItem::Tessellation { tessellation } => {
                    index.tessellations.contains_key(tessellation.as_str())
                }
                PresentationItem::Source { source_id } => !source_id.is_empty(),
            };
            if !resolved {
                invalid_layer(
                    findings,
                    layer.id.as_str(),
                    "unresolved presentation-layer item",
                );
            }
        }
    }
}

fn invalid_state(findings: &mut Vec<Finding>, entity: Option<String>, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity,
    });
}

fn invalid_layer(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::Presentation,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
