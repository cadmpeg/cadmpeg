// SPDX-License-Identifier: Apache-2.0
//! Drawing graph and numeric validation.

use std::collections::HashSet;

use crate::document::CadIr;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_drawings(
    ir: &CadIr,
    all_ids: &crate::index::ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let mut orders = HashSet::new();
    for drawing in &ir.model.drawings {
        let refs_valid = all_ids.contains(&drawing.object)
            && all_ids.contains(&drawing.native_ref)
            && drawing
                .template
                .as_ref()
                .is_none_or(|id| all_ids.contains(id.as_str()))
            && drawing.assets.iter().all(|id| all_ids.contains(id))
            && drawing
                .relationships
                .values()
                .flatten()
                .all(|target| target.local_target().is_none_or(|id| all_ids.contains(id)));
        let order_valid = orders.insert(drawing.order);
        if !refs_valid || !order_valid {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "invalid drawing reference, order, or numeric state".into(),
                entity: Some(drawing.id.as_str().to_owned()),
            });
        }
    }
}
