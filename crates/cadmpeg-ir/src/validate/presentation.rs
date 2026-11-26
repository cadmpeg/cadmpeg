// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use std::collections::{BTreeMap, BTreeSet};

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
                crate::rule!(
                    "presentation.invalid-document-presentation-state",
                    "{}",
                    "invalid document presentation state"
                ),
            );
        }
    }

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
        if !references_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(view.id.as_str()),
                crate::rule!(
                    "presentation.invalid-view-presentation-reference",
                    "{}",
                    "invalid view presentation reference"
                ),
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
    let point_clouds = BorrowedIdentities::build(|add| {
        for item in &ir.model.point_clouds {
            add(item.id.as_str(), ());
        }
    });
    let layers = BorrowedIdentities::build(|add| {
        for layer in &ir.model.presentation_layers {
            add(layer.id.as_str(), ());
        }
    });
    let mut parents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for layer in &ir.model.presentation_layers {
        if let Some(parent) = &layer.parent {
            if layers.contains(parent.as_str()) {
                parents
                    .entry(layer.id.as_str())
                    .or_default()
                    .insert(parent.as_str());
            } else {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(layer.id.as_str()),
                    crate::rule!(
                        "presentation.unresolved-presentation-layer-parent",
                        "{}",
                        "unresolved presentation-layer parent"
                    ),
                );
            }
        }
        for item in &layer.items {
            let resolved = match &item.entity {
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
                PresentationItem::PointCloud { point_cloud } => {
                    point_clouds.contains(point_cloud.as_str())
                }
                PresentationItem::Source { .. } => true,
            };
            if !resolved {
                record_finding(
                    findings,
                    Check::Presentation,
                    crate::report::Severity::Error,
                    Some(layer.id.as_str()),
                    crate::rule!(
                        "presentation.unresolved-presentation-layer-item",
                        "{}",
                        "unresolved presentation-layer item"
                    ),
                );
            }
        }
    }
    for layer in super::cycles::on_cycles(&parents) {
        record_finding(
            findings,
            Check::Presentation,
            Severity::Error,
            Some(layer),
            crate::rule!(
                "presentation.presentation-layer-own-ancestor-through-parent",
                "presentation layer is its own ancestor through `parent`"
            ),
        );
    }
}
