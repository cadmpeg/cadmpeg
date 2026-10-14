// SPDX-License-Identifier: Apache-2.0
//! Drawing graph and numeric validation.

use super::record_finding;
use crate::index::identities::BorrowedIdentities;

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_drawings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    all_ids: &BorrowedIdentities<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut orders = std::collections::BTreeSet::new();
    for drawing in &ir.model.drawings {
        let mut refs_valid = all_ids.contains(ctx, &drawing.object)?
            && all_ids.contains(ctx, &drawing.native_ref)?;
        if refs_valid {
            if let Some(id) = &drawing.template {
                refs_valid = all_ids.contains(ctx, id.as_str())?;
            }
        }
        if refs_valid {
            for id in &drawing.assets {
                if !all_ids.contains(ctx, id)? {
                    refs_valid = false;
                    break;
                }
            }
        }
        if refs_valid {
            'groups: for targets in drawing.relationships.values() {
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
        let order_valid = orders.insert(drawing.order);
        if !refs_valid || !order_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(drawing.id.as_str()),
                format_args!("invalid drawing reference, order, or numeric state"),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_drawings;
    use crate::document::CadIr;

    fn fixture() -> CadIr {
        let mut ir = CadIr::empty();
        for id in ["test:model:drawing#first", "test:model:drawing#second"] {
            ir.model.drawings.push(
                serde_json::from_value(serde_json::json!({
                    "id": id, "object": "test:source:unknown#record", "kind": "page",
                    "runtime_type": "Test", "order": 7, "native_ref": "test:source:unknown#record"
                }))
                .unwrap(),
            );
        }
        ir
    }

    #[test]
    fn drawings_report_the_second_duplicate_order() {
        let ir = fixture();
        let ctx = cadmpeg_test_support::service_decode_context();
        let ids =
            super::BorrowedIdentities::build(&ctx, |add| add("test:source:unknown#record", ()))
                .unwrap();
        let mut findings = Vec::new();
        check_drawings(&ctx, &ir, &ids, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].entity.as_deref(),
            Some("test:model:drawing#second")
        );
        assert_eq!(
            findings[0].check,
            crate::report::check::Check::ReferentialIntegrity
        );
        assert_eq!(
            findings[0].message,
            "invalid drawing reference, order, or numeric state"
        );
    }
}
