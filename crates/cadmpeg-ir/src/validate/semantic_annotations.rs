// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph and numeric validation.

use super::{identities::BorrowedIdentities, orders::Orders, record_finding};

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_semantic_annotations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    all_ids: &BorrowedIdentities<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut orders = Orders::new(ctx)?;
    for annotation in &ir.model.semantic_annotations {
        ctx.charge_work(1, "semantic annotation row scan")?;
        let mut refs_valid = all_ids.contains(ctx, &annotation.object)?
            && all_ids.contains(ctx, &annotation.native_ref)?;
        if refs_valid {
            for id in &annotation.assets {
                ctx.charge_work(1, "semantic annotation asset scan")?;
                if !all_ids.contains(ctx, id)? { refs_valid = false; break; }
            }
        }
        if refs_valid {
            'groups: for targets in annotation.references.values() {
                ctx.charge_work(1, "semantic annotation reference group scan")?;
                for target in targets {
                    ctx.charge_work(1, "semantic annotation reference scan")?;
                    if let Some(id) = target.local_target() {
                        if !all_ids.contains(ctx, id)? { refs_valid = false; break 'groups; }
                    }
                }
            }
        }
        let order_valid = orders.insert(annotation.order)?;
        if !refs_valid || !order_valid {
            record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error,
                Some(annotation.id.as_str()), format_args!("invalid semantic annotation reference, order, or numeric state"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_semantic_annotations;
    use crate::document::CadIr;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn fixture() -> CadIr {
        let mut ir = CadIr::empty();
        for id in ["test:model:semantic-annotation#first", "test:model:semantic-annotation#second"] {
            ir.model.semantic_annotations.push(serde_json::from_value(serde_json::json!({
                "id": id, "object": "test:source:unknown#record", "kind": "text",
                "runtime_type": "Test", "order": 7, "native_ref": "test:source:unknown#record"
            })).unwrap());
        }
        ir
    }

    #[test]
    fn semantic_annotations_preserve_scan_order_and_finding_resource_refusals() {
        let ir = fixture();
        let source = cadmpeg_test_support::service_decode_context();
        let ids = super::BorrowedIdentities::build(&source, |add| add("test:source:unknown#record", ())).unwrap();
        for dimension in [ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let Err(CodecError::ResourceLimit(limit)) = check_semantic_annotations(&ctx, &ir, &ids, &mut findings) else { panic!("validation must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert!(findings.is_empty());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }

    #[test]
    fn semantic_annotations_report_the_second_duplicate_order_and_release_storage() {
        let ir = fixture();
        let source = cadmpeg_test_support::service_decode_context();
        let ids = super::BorrowedIdentities::build(&source, |add| add("test:source:unknown#record", ())).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        check_semantic_annotations(&ctx, &ir, &ids, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity.as_deref(), Some("test:model:semantic-annotation#second"));
        assert_eq!(findings[0].check, crate::report::check::Check::ReferentialIntegrity);
        assert_eq!(findings[0].message, "invalid semantic annotation reference, order, or numeric state");
        drop(ctx.reserve_scoped(4096, "order storage released").unwrap());
        ctx.finish_session().unwrap();
    }
}
