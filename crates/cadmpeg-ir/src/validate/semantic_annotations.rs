// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph and numeric validation.

use std::collections::HashSet;

use crate::document::CadIr;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_semantic_annotations(
    ir: &CadIr,
    all_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    let mut orders = HashSet::new();
    for annotation in &ir.model.semantic_annotations {
        let refs_valid = all_ids.contains(&annotation.object)
            && all_ids.contains(&annotation.native_ref)
            && annotation.assets.iter().all(|id| all_ids.contains(id))
            && annotation.references.values().flatten().all(|target| {
                target.target.as_ref().is_none_or(|id| all_ids.contains(id))
                    && (target.is_null
                        || target.target.is_some()
                        || (target.external_document.is_some() && target.external_object.is_some()))
            });
        let numeric_valid = annotation
            .value
            .iter()
            .chain(annotation.position.iter().flatten())
            .all(|value| value.is_finite());
        let order_valid = orders.insert(annotation.order);
        if !refs_valid || !numeric_valid || !order_valid {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "invalid semantic annotation reference, order, or numeric state".into(),
                entity: Some(annotation.id.0.clone()),
            });
        }
    }
}
