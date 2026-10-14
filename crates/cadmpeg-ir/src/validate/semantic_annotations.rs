// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph and numeric validation.

use super::record_finding;
use crate::index::identities::BorrowedIdentities;

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
    let mut orders = std::collections::BTreeSet::new();
    for annotation in &ir.model.semantic_annotations {
        let mut refs_valid = all_ids.contains(ctx, &annotation.object)?
            && all_ids.contains(ctx, &annotation.native_ref)?;
        if refs_valid {
            for id in &annotation.assets {
                if !all_ids.contains(ctx, id)? {
                    refs_valid = false;
                    break;
                }
            }
        }
        if refs_valid {
            'groups: for targets in annotation.references.values() {
                for target in targets {
                    if let Some(id) = target.local_target() {
                        if !all_ids.contains(ctx, id)? {
                            refs_valid = false;
                            break 'groups;
                        }
                    }
                }
            }
        }
        let order_valid = orders.insert(annotation.order);
        if !refs_valid || !order_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(annotation.id.as_str()),
                format_args!("invalid semantic annotation reference, order, or numeric state"),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_semantic_annotations;
    use crate::document::CadIr;

    fn fixture() -> CadIr {
        let mut ir = CadIr::empty();
        for id in [
            "test:model:semantic-annotation#first",
            "test:model:semantic-annotation#second",
        ] {
            ir.model.semantic_annotations.push(
                serde_json::from_value(serde_json::json!({
                    "id": id, "object": "test:source:unknown#record", "kind": "text",
                    "runtime_type": "Test", "order": 7, "native_ref": "test:source:unknown#record"
                }))
                .unwrap(),
            );
        }
        ir
    }

    #[test]
    fn semantic_annotations_report_the_second_duplicate_order() {
        let ir = fixture();
        let ctx = cadmpeg_test_support::service_decode_context();
        let ids =
            super::BorrowedIdentities::build(&ctx, |add| add("test:source:unknown#record", ()))
                .unwrap();
        let mut findings = Vec::new();
        check_semantic_annotations(&ctx, &ir, &ids, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].entity.as_deref(),
            Some("test:model:semantic-annotation#second")
        );
        assert_eq!(
            findings[0].check,
            crate::report::check::Check::ReferentialIntegrity
        );
        assert_eq!(
            findings[0].message,
            "invalid semantic annotation reference, order, or numeric state"
        );
    }
}
