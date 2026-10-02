// SPDX-License-Identifier: Apache-2.0
//! Presentation state and layer validation.

use super::{identities::BorrowedIdentities, orders::Orders, record_finding};
use crate::document::CadIr;
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
        record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, None, format_args!("{}", "multiple document presentation records"))?;
    }
    for document in &ir.model.presentation_documents {
        ctx.charge_work(1, "presentation document scan")?;
        let native_valid = match &document.native_ref {
            Some(native) => all_ids.contains(ctx, native)?,
            None => true,
        };
        let mut assets_valid = true;
        'states: for state in document.states() {
            ctx.charge_work(1, "presentation state scan")?;
            for asset in &state.assets {
                ctx.charge_work(1, "presentation asset scan")?;
                if !all_ids.contains(ctx, asset)? { assets_valid = false; break 'states; }
            }
        }
        if !native_valid || !assets_valid {
            record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(document.id.as_str()), format_args!("{}", "invalid document presentation state"))?;
        }
    }

    let mut orders = Orders::new(ctx)?;
    for view in &ir.model.view_presentations {
        ctx.charge_work(1, "view presentation scan")?;
        let mut references_valid = match &view.object {
            Some(object) => all_ids.contains(ctx, object)?,
            None => true,
        };
        if references_valid {
            if let Some(native) = &view.native_ref { references_valid = all_ids.contains(ctx, native)?; }
        }
        if !references_valid || !orders.insert(view.order)? {
            record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(view.id.as_str()), format_args!("{}", "invalid view presentation reference, order, or size"))?;
        }
    }

    let bodies = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.bodies { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let faces = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.faces { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let edges = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.edges { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let vertices = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.vertices { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let points = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.points { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let curves = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.curves { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let surfaces = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.surfaces { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let products = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.product_definitions { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let occurrences = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.occurrences { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let pmi = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.pmi { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    let tessellations = BorrowedIdentities::build(ctx, |add| {
        for item in &ir.model.tessellations { add(item.id.as_str(), ())?; }
        Ok(())
    })?;
    for layer in &ir.model.presentation_layers {
        ctx.charge_work(1, "presentation layer scan")?;
        for item in &layer.items {
            ctx.charge_work(1, "presentation layer item scan")?;
            let resolved = match item {
                PresentationItem::Body { body } => bodies.contains(ctx, body.as_str())?,
                PresentationItem::Face { face } => faces.contains(ctx, face.as_str())?,
                PresentationItem::Edge { edge } => edges.contains(ctx, edge.as_str())?,
                PresentationItem::Vertex { vertex } => vertices.contains(ctx, vertex.as_str())?,
                PresentationItem::Point { point } => points.contains(ctx, point.as_str())?,
                PresentationItem::Curve { curve } => curves.contains(ctx, curve.as_str())?,
                PresentationItem::Surface { surface } => surfaces.contains(ctx, surface.as_str())?,
                PresentationItem::Product { product } => products.contains(ctx, product.as_str())?,
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
                record_finding(ctx, findings, Check::Presentation, crate::report::Severity::Error, Some(layer.id.as_str()), format_args!("{}", "unresolved presentation-layer item"))?;
            }
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    #[test]
    fn presentation_typed_indexes_preserve_resource_refusals_and_release_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let mut ir = crate::CadIr::empty();
        ir.model.points.push(crate::topology::Point::new(
            "test:model:point#source".try_into().unwrap(), crate::features::FinitePoint3::ZERO, None));
        let source = cadmpeg_test_support::service_decode_context();
        let ids = super::BorrowedIdentities::build(&source, |_| Ok(())).unwrap();
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let Err(CodecError::ResourceLimit(limit)) = super::check_presentation(&ctx, &ir, &ids, &mut findings) else { panic!("presentation index must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert!(findings.is_empty());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 8192;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        super::check_presentation(&ctx, &ir, &ids, &mut findings).unwrap();
        assert!(findings.is_empty());
        drop(ctx.reserve_scoped(8192, "presentation indexes released").unwrap());
        ctx.finish_session().unwrap();
    }
}
