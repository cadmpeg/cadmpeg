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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    all_ids: &BorrowedIdentities<'_, '_>,
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
            Some(native) => all_ids.contains(ctx, native)?,
            None => true,
        };
        let mut assets_valid = true;
        'states: for state in document.states() {
            for asset in &state.assets {
                if !all_ids.contains(ctx, asset)? {
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
            Some(object) => all_ids.contains(ctx, object)?,
            None => true,
        };
        if references_valid {
            if let Some(native) = &view.native_ref {
                references_valid = all_ids.contains(ctx, native)?;
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

    let bodies = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.bodies {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let faces = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.faces {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let edges = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.edges {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let vertices = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.vertices {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let points = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.points {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let curves = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.curves {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let surfaces = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.surfaces {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let products = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.product_definitions {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let occurrences = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.occurrences {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let pmi = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.pmi {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let tessellations = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.tessellations {
            add(item.id.as_str(), ())?;
        }
        Ok(())
    })?;
    for layer in &ir.model.presentation_layers {
        for item in &layer.items {
            let resolved = match item {
                PresentationItem::Body { body } => bodies.contains(ctx, body.as_str())?,
                PresentationItem::Face { face } => faces.contains(ctx, face.as_str())?,
                PresentationItem::Edge { edge } => edges.contains(ctx, edge.as_str())?,
                PresentationItem::Vertex { vertex } => vertices.contains(ctx, vertex.as_str())?,
                PresentationItem::Point { point } => points.contains(ctx, point.as_str())?,
                PresentationItem::Curve { curve } => curves.contains(ctx, curve.as_str())?,
                PresentationItem::Surface { surface } => {
                    surfaces.contains(ctx, surface.as_str())?
                }
                PresentationItem::Product { product } => {
                    products.contains(ctx, product.as_str())?
                }
                PresentationItem::Occurrence { occurrence } => {
                    occurrences.contains(ctx, occurrence.as_str())?
                }
                PresentationItem::Pmi { annotation } => pmi.contains(ctx, annotation.as_str())?,
                PresentationItem::Tessellation { tessellation } => {
                    tessellations.contains(ctx, tessellation.as_str())?
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
