// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::{BorrowedIdentities, IdentitySet};
use crate::index::{ModelIndex, StandardIndex};
use crate::presentation::PresentationItem;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_presentation(ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    let ir: &CadIr = ids.ir();
    let all_ids: &dyn IdentitySet = ids;
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

    if ir.model.presentation_layers.is_empty() {
        return;
    }
    let products = BorrowedIdentities::build(|add| {
        for item in &ir.model.product_definitions {
            add(item.id.as_str(), ());
        }
    });
    let occurrences = BorrowedIdentities::build(|add| {
        for item in &ir.model.occurrences {
            add(item.id.as_str(), ());
        }
    });
    let pmi = BorrowedIdentities::build(|add| {
        for item in &ir.model.pmi {
            add(item.id.as_str(), ());
        }
    });
    for layer in &ir.model.presentation_layers {
        for item in &layer.items {
            let resolved = match item {
                PresentationItem::Body { body } => {
                    ids.bodies(body.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Face { face } => {
                    ids.faces(face.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Edge { edge } => {
                    ids.edges(edge.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Vertex { vertex } => {
                    ids.vertices(vertex.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Point { point } => {
                    ids.points(point.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Curve { curve } => {
                    ids.curves(curve.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Surface { surface } => {
                    ids.surfaces(surface.as_str(), StandardIndex).is_some()
                }
                PresentationItem::Product { product } => products.contains(product.as_str()),
                PresentationItem::Occurrence { occurrence } => {
                    occurrences.contains(occurrence.as_str())
                }
                PresentationItem::Pmi { annotation } => pmi.contains(annotation.as_str()),
                PresentationItem::Tessellation { tessellation } => ids
                    .tessellations(tessellation.as_str(), StandardIndex)
                    .is_some(),
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
}
