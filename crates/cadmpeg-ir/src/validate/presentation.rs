// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::presentation::PresentationItem;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_presentation(
    ir: &CadIr,
    all_ids: &BorrowedIdentities<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    if ir.model.presentation_documents.len() > 1 {
        record_finding(
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            None,
            format_args!("{}", "multiple document presentation records"),
        );
    }
    for document in &ir.model.presentation_documents {
        let native_valid = match &document.native_ref {
            Some(native) => all_ids.contains(native),
            None => true,
        };
        let mut assets_valid = true;
        'states: for state in document.states() {
            for asset in &state.assets {
                if !all_ids.contains(asset) {
                    assets_valid = false;
                    break 'states;
                }
            }
        }
        if !native_valid || !assets_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(document.id.as_str()),
                format_args!("{}", "invalid document presentation state"),
            );
        }
    }

    let mut orders = std::collections::BTreeSet::new();
    for view in &ir.model.view_presentations {
        let mut references_valid = match &view.object {
            Some(object) => all_ids.contains(object),
            None => true,
        };
        if references_valid {
            if let Some(native) = &view.native_ref {
                references_valid = all_ids.contains(native);
            }
        }
        if !references_valid || !orders.insert(view.order) {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(view.id.as_str()),
                format_args!("{}", "invalid view presentation reference, order, or size"),
            );
        }
    }

    let bodies = BorrowedIdentities::build(|add| {
        for item in &ir.model.bodies {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let faces = BorrowedIdentities::build(|add| {
        for item in &ir.model.faces {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let edges = BorrowedIdentities::build(|add| {
        for item in &ir.model.edges {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let vertices = BorrowedIdentities::build(|add| {
        for item in &ir.model.vertices {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let points = BorrowedIdentities::build(|add| {
        for item in &ir.model.points {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let curves = BorrowedIdentities::build(|add| {
        for item in &ir.model.curves {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let surfaces = BorrowedIdentities::build(|add| {
        for item in &ir.model.surfaces {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let products = BorrowedIdentities::build(|add| {
        for item in &ir.model.product_definitions {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let occurrences = BorrowedIdentities::build(|add| {
        for item in &ir.model.occurrences {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let pmi = BorrowedIdentities::build(|add| {
        for item in &ir.model.pmi {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let tessellations = BorrowedIdentities::build(|add| {
        for item in &ir.model.tessellations {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
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
                record_finding(
                    findings,
                    Check::Presentation,
                    crate::report::Severity::Error,
                    Some(layer.id.as_str()),
                    format_args!("{}", "unresolved presentation-layer item"),
                );
            }
        }
    }
    Ok(())
}
