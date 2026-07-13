// SPDX-License-Identifier: Apache-2.0
//! Presentation-layer membership validation.

use crate::document::CadIr;
use crate::presentation::PresentationItem;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_presentation(ir: &CadIr, findings: &mut Vec<Finding>) {
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
                PresentationItem::Body { body } => ir
                    .model
                    .bodies
                    .iter()
                    .any(|candidate| candidate.id == *body),
                PresentationItem::Face { face } => {
                    ir.model.faces.iter().any(|candidate| candidate.id == *face)
                }
                PresentationItem::Edge { edge } => {
                    ir.model.edges.iter().any(|candidate| candidate.id == *edge)
                }
                PresentationItem::Vertex { vertex } => ir
                    .model
                    .vertices
                    .iter()
                    .any(|candidate| candidate.id == *vertex),
                PresentationItem::Point { point } => ir
                    .model
                    .points
                    .iter()
                    .any(|candidate| candidate.id == *point),
                PresentationItem::Curve { curve } => ir
                    .model
                    .curves
                    .iter()
                    .any(|candidate| candidate.id == *curve),
                PresentationItem::Surface { surface } => ir
                    .model
                    .surfaces
                    .iter()
                    .any(|candidate| candidate.id == *surface),
                PresentationItem::Product { product } => ir
                    .model
                    .products
                    .iter()
                    .any(|candidate| candidate.id == *product),
                PresentationItem::Occurrence { occurrence } => ir
                    .model
                    .occurrences
                    .iter()
                    .any(|candidate| candidate.id == *occurrence),
                PresentationItem::Pmi { annotation } => ir
                    .model
                    .pmi
                    .iter()
                    .any(|candidate| candidate.id == *annotation),
                PresentationItem::Tessellation { tessellation } => ir
                    .model
                    .tessellations
                    .iter()
                    .any(|candidate| candidate.id == *tessellation),
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

fn invalid(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::Presentation,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
