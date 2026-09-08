// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use std::collections::HashSet;

use crate::document::CadIr;
use crate::presentation::PresentationItem;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_presentation(
    ir: &CadIr,
    all_ids: &crate::index::ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    if ir.model.presentation_documents.len() > 1 {
        invalid_state(findings, None, "multiple document presentation records");
    }
    for document in &ir.model.presentation_documents {
        let native_valid = document
            .native_ref
            .as_ref()
            .is_none_or(|native| all_ids.contains(native));
        let assets_valid = document
            .states()
            .iter()
            .flat_map(|state| &state.assets)
            .all(|asset| all_ids.contains(asset));
        if !native_valid || !assets_valid {
            invalid_state(
                findings,
                Some(document.id.as_str().to_owned()),
                "invalid document presentation state",
            );
        }
    }

    let mut orders = HashSet::new();
    for view in &ir.model.view_presentations {
        let references_valid = view
            .object
            .as_ref()
            .is_none_or(|object| all_ids.contains(object))
            && view
                .native_ref
                .as_ref()
                .is_none_or(|native| all_ids.contains(native));
        if !references_valid || !orders.insert(view.order) {
            invalid_state(
                findings,
                Some(view.id.as_str().to_owned()),
                "invalid view presentation reference, order, or size",
            );
        }
    }

    let bodies = ids(&ir.model.bodies, |item| item.id.as_str());
    let faces = ids(&ir.model.faces, |item| item.id.as_str());
    let edges = ids(&ir.model.edges, |item| item.id.as_str());
    let vertices = ids(&ir.model.vertices, |item| item.id.as_str());
    let points = ids(&ir.model.points, |item| item.id.as_str());
    let curves = ids(&ir.model.curves, |item| item.id.as_str());
    let surfaces = ids(&ir.model.surfaces, |item| item.id.as_str());
    let products = ids(&ir.model.product_definitions, |item| item.id.as_str());
    let occurrences = ids(&ir.model.occurrences, |item| item.id.as_str());
    let pmi = ids(&ir.model.pmi, |item| item.id.as_str());
    let tessellations = ids(&ir.model.tessellations, |item| item.id.as_str());
    for layer in &ir.model.presentation_layers {
        for item in &layer.items {
            let resolved = match item {
                PresentationItem::Body { body } => bodies.contains(body.as_str()),
                PresentationItem::Face { face } => faces.contains(face.as_str()),
                PresentationItem::Edge { edge } => edges.contains(edge.as_str()),
                PresentationItem::Vertex { vertex } => vertices.contains(vertex.as_str()),
                PresentationItem::Point { point } => points.contains(point.as_str()),
                PresentationItem::Curve { curve } => curves.contains(curve.as_str()),
                PresentationItem::Surface { surface } => surfaces.contains(surface.as_str()),
                PresentationItem::Product { product } => products.contains(product.as_str()),
                PresentationItem::Occurrence { occurrence } => {
                    occurrences.contains(occurrence.as_str())
                }
                PresentationItem::Pmi { annotation } => pmi.contains(annotation.as_str()),
                PresentationItem::Tessellation { tessellation } => {
                    tessellations.contains(tessellation.as_str())
                }
                PresentationItem::Source { .. } => true,
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

fn ids<'a, T>(items: &'a [T], id: impl Fn(&'a T) -> &'a str) -> HashSet<&'a str> {
    items.iter().map(id).collect()
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
