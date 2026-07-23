// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph and numeric validation.

use std::collections::HashSet;

use super::{Check, Finding, ModelIndex, Severity};
use crate::document::CadIr;

pub(super) fn check_semantic_annotations(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let mut orders = HashSet::new();
    for annotation in &ir.model.semantic_annotations {
        let refs_valid = index.contains(&annotation.object)
            && index.contains(&annotation.native_ref)
            && annotation.assets.iter().all(|id| index.contains(id))
            && annotation.references.values().flatten().all(|target| {
                target.target.as_ref().is_none_or(|id| index.contains(id))
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
