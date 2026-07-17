// SPDX-License-Identifier: Apache-2.0
//! Presentation-layer membership validation.

use crate::document::CadIr;
use crate::presentation::PresentationItem;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_presentation(ir: &CadIr, findings: &mut Vec<Finding>) {
    let bodies = ids(&ir.model.bodies, |item| item.id.as_str());
    let faces = ids(&ir.model.faces, |item| item.id.as_str());
    let edges = ids(&ir.model.edges, |item| item.id.as_str());
    let vertices = ids(&ir.model.vertices, |item| item.id.as_str());
    let points = ids(&ir.model.points, |item| item.id.as_str());
    let curves = ids(&ir.model.curves, |item| item.id.as_str());
    let surfaces = ids(&ir.model.surfaces, |item| item.id.as_str());
    let products = ids(&ir.model.products, |item| item.id.as_str());
    let occurrences = ids(&ir.model.occurrences, |item| item.id.as_str());
    let pmi = ids(&ir.model.pmi, |item| item.id.as_str());
    let tessellations = ids(&ir.model.tessellations, |item| item.id.as_str());
    for layer in &ir.model.presentation_layers {
        if layer.name.is_empty() {
            invalid(
                findings,
                layer.id.as_str(),
                "presentation layer has no name",
            );
        }
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
                PresentationItem::Source { source_id } => !source_id.is_empty(),
            };
            if !resolved {
                invalid(
                    findings,
                    layer.id.as_str(),
                    "unresolved presentation-layer item",
                );
            }
        }
    }
}

fn ids<'a, T>(items: &'a [T], id: impl Fn(&'a T) -> &'a str) -> std::collections::HashSet<&'a str> {
    items.iter().map(id).collect()
}

fn invalid(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::Presentation,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
