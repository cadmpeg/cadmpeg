// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

use super::{identities::BorrowedIdentities, record_finding};
use crate::document::CadIr;
use crate::pmi::{PmiDefinition, PmiTarget};
use crate::report::check::{Check, Finding};

pub(super) fn check_pmi(ctx: &cadmpeg_core::decode::DecodeContext<'_>, ir: &CadIr, findings: &mut Vec<Finding>) -> Result<(), cadmpeg_core::CodecError> {
    let definitions = BorrowedIdentities::build(ctx, |add| {
        for annotation in &ir.model.pmi { add(annotation.id.as_str(), &annotation.definition)?; }
        Ok(())
    })?;
    macro_rules! typed_identities {
        ($arena:ident) => {
            BorrowedIdentities::build(ctx, |add| {
                for item in &ir.model.$arena { add(item.id.as_str(), ())?; }
                Ok(())
            })?
        };
    }
    let bodies = typed_identities!(bodies);
    let faces = typed_identities!(faces);
    let edges = typed_identities!(edges);
    let vertices = typed_identities!(vertices);
    let points = typed_identities!(points);
    let curves = typed_identities!(curves);
    let products = typed_identities!(product_definitions);
    let occurrences = typed_identities!(occurrences);
    for annotation in &ir.model.pmi {
        ctx.charge_work(1, "PMI annotation scan")?;
        for target in &annotation.targets {
            ctx.charge_work(1, "PMI target scan")?;
            let resolved = match target {
                PmiTarget::Body { body } => bodies.contains(ctx, body.as_str())?,
                PmiTarget::Face { face } => faces.contains(ctx, face.as_str())?,
                PmiTarget::Edge { edge } => edges.contains(ctx, edge.as_str())?,
                PmiTarget::Vertex { vertex } => vertices.contains(ctx, vertex.as_str())?,
                PmiTarget::Point { point } => points.contains(ctx, point.as_str())?,
                PmiTarget::Curve { curve } => curves.contains(ctx, curve.as_str())?,
                PmiTarget::Product { product } => products.contains(ctx, product.as_str())?,
                PmiTarget::Occurrence { occurrence } => occurrences.contains(ctx, occurrence.as_str())?,
                PmiTarget::ShapeAspect { .. } => true,
            };
            if !resolved {
                record_finding(ctx, findings, Check::Pmi, crate::report::Severity::Error, Some(annotation.id.as_str()), format_args!("{}", "unresolved PMI target"))?;
            }
        }
        match &annotation.definition {
            PmiDefinition::DatumSystem { references } => {
                for reference in references.as_slice() {
                    ctx.charge_work(1, "PMI datum reference scan")?;
                    if !matches!(
                        definitions.get(ctx, reference.datum.as_str())?.copied(),
                        Some(PmiDefinition::Datum { .. })
                    ) {
                        record_finding(ctx, findings, Check::Pmi, crate::report::Severity::Error, Some(annotation.id.as_str()), format_args!("{}", "unresolved datum reference"))?;
                    }
                }
            }
            PmiDefinition::GeometricTolerance { datum_system, .. } => {
                if let Some(id) = datum_system {
                    if !matches!(definitions.get(ctx, id.as_str())?.copied(), Some(PmiDefinition::DatumSystem { .. })) {
                        record_finding(ctx, findings, Check::Pmi, crate::report::Severity::Error, Some(annotation.id.as_str()), format_args!("unresolved datum system"))?;
                    }
                }
            }
            PmiDefinition::Dimension(_) => {}
            PmiDefinition::Presentation { semantics, .. } => {
                for id in semantics {
                    ctx.charge_work(1, "PMI semantic reference scan")?;
                    if !definitions.contains(ctx, id.as_str())? {
                        record_finding(ctx, findings, Check::Pmi, crate::report::Severity::Error, Some(annotation.id.as_str()), format_args!("unresolved semantic annotation"))?;
                        break;
                    }
                }
            }
            PmiDefinition::Datum { .. } => {}
            PmiDefinition::DatumTarget { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_pmi;
    use crate::document::CadIr;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn pmi_indexes_preserve_caller_refusal_and_release_borrowed_storage() {
        let mut ir = CadIr::empty();
        ir.model.points.push(crate::topology::Point::new(
            "test:model:point#source".try_into().unwrap(), crate::features::FinitePoint3::ZERO, None));
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
            let Err(CodecError::ResourceLimit(limit)) = check_pmi(&ctx, &ir, &mut findings) else { panic!("PMI index must refuse"); };
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
        check_pmi(&ctx, &ir, &mut findings).unwrap();
        assert!(findings.is_empty());
        drop(ctx.reserve_scoped(8192, "PMI indexes released").unwrap());
        ctx.finish_session().unwrap();
    }
}
