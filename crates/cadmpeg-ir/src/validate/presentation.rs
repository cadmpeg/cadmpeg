// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use super::{orders::Orders, record_finding};
use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::presentation::PresentationItem;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_presentation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    all_ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    if ir.model.presentation_documents.len() > 1 {
        record_finding(
            ctx,
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            None,
            format_args!("{}", "multiple document presentation records"),
        )?;
    }
    for document in &ir.model.presentation_documents {
        ctx.charge_work(1, "presentation document scan")?;
        let native_valid = match &document.native_ref {
            Some(native) => all_ids.contains(native.as_str(), ctx)?,
            None => true,
        };
        let mut assets_valid = true;
        'states: for state in document.states() {
            ctx.charge_work(1, "presentation state scan")?;
            for asset in &state.assets {
                ctx.charge_work(1, "presentation asset scan")?;
                if !all_ids.contains(asset.as_str(), ctx)? {
                    assets_valid = false;
                    break 'states;
                }
            }
        }
        if !native_valid || !assets_valid {
            record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(document.id.as_str()),
                format_args!("{}", "invalid document presentation state"),
            )?;
        }
    }

    let mut orders = Orders::new(ctx)?;
    for view in &ir.model.view_presentations {
        ctx.charge_work(1, "view presentation scan")?;
        let mut references_valid = match &view.object {
            Some(object) => all_ids.contains(object.as_str(), ctx)?,
            None => true,
        };
        if references_valid {
            if let Some(native) = &view.native_ref {
                references_valid = all_ids.contains(native.as_str(), ctx)?;
            }
        }
        if !references_valid || !orders.insert(view.order)? {
            record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(view.id.as_str()),
                format_args!("{}", "invalid view presentation reference, order, or size"),
            )?;
        }
    }

    for layer in &ir.model.presentation_layers {
        ctx.charge_work(1, "presentation layer scan")?;
        for item in &layer.items {
            ctx.charge_work(1, "presentation layer item scan")?;
            let resolved = match item {
                PresentationItem::Body { body } => all_ids.bodies(body.as_str(), ctx)?.is_some(),
                PresentationItem::Face { face } => all_ids.faces(face.as_str(), ctx)?.is_some(),
                PresentationItem::Edge { edge } => all_ids.edges(edge.as_str(), ctx)?.is_some(),
                PresentationItem::Vertex { vertex } => {
                    all_ids.vertices(vertex.as_str(), ctx)?.is_some()
                }
                PresentationItem::Point { point } => all_ids.points(point.as_str(), ctx)?.is_some(),
                PresentationItem::Curve { curve } => all_ids.curves(curve.as_str(), ctx)?.is_some(),
                PresentationItem::Surface { surface } => {
                    all_ids.surfaces(surface.as_str(), ctx)?.is_some()
                }
                PresentationItem::Product { product } => all_ids
                    .product_definitions(product.as_str(), ctx)?
                    .is_some(),
                PresentationItem::Occurrence { occurrence } => {
                    all_ids.occurrences(occurrence.as_str(), ctx)?.is_some()
                }
                PresentationItem::Pmi { annotation } => {
                    all_ids.pmi(annotation.as_str(), ctx)?.is_some()
                }
                PresentationItem::Tessellation { tessellation } => {
                    all_ids.tessellations(tessellation.as_str(), ctx)?.is_some()
                }
                PresentationItem::Source { .. } => true,
            };
            if !resolved {
                record_finding(
                    ctx,
                    findings,
                    Check::Presentation,
                    crate::report::Severity::Error,
                    Some(layer.id.as_str()),
                    format_args!("{}", "unresolved presentation-layer item"),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::index::ModelIndex;
    use crate::presentation::{PresentationItem, PresentationLayer};

    fn layered_ir() -> crate::CadIr {
        let mut ir = crate::CadIr::empty();
        ir.model.points.push(crate::topology::Point::new(
            "test:model:point#source".try_into().unwrap(),
            crate::features::FinitePoint3::ZERO,
            None,
        ));
        ir.model.presentation_layers.push(PresentationLayer {
            id: crate::ids::LayerId::mint("test:presentation:layer#items").unwrap(),
            name: "items".into(),
            description: None,
            visible: None,
            items: vec![
                PresentationItem::Point {
                    point: "test:model:point#source".try_into().unwrap(),
                },
                PresentationItem::Face {
                    face: "test:model:point#source".try_into().unwrap(),
                },
            ],
        });
        ir
    }

    #[test]
    fn presentation_items_resolve_in_their_own_arena() {
        let ir = layered_ir();
        let ctx = cadmpeg_test_support::service_decode_context();
        let ids = ModelIndex::build(&ir, &ctx).unwrap();
        let mut findings = Vec::new();
        super::check_presentation(&ctx, &ir, &ids, &mut findings).unwrap();
        // The point resolves; the same identity named as a face does not.
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].entity.as_deref(),
            Some("test:presentation:layer#items")
        );
    }

    #[test]
    fn presentation_item_lookup_preserves_work_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let ir = layered_ir();
        let service = cadmpeg_test_support::service_decode_context();
        let ids = ModelIndex::build(&ir, &service).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) =
            super::check_presentation(&ctx, &ir, &ids, &mut findings)
        else {
            panic!("presentation lookup must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}
